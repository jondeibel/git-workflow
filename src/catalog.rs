use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::state::{self, StackConfig};
use crate::validate;

#[derive(Debug, Clone)]
pub struct CatalogIssue {
    pub message: String,
    pub repairable: bool,
}

pub struct StackCatalog {
    stacks_dir: PathBuf,
    stacks: Vec<StackConfig>,
    load_issues: Vec<CatalogIssue>,
}

impl StackCatalog {
    pub fn open(stacks_dir: &Path) -> Result<Self> {
        if !stacks_dir.exists() {
            return Ok(Self {
                stacks_dir: stacks_dir.to_path_buf(),
                stacks: vec![],
                load_issues: vec![],
            });
        }

        let entries = std::fs::read_dir(stacks_dir)
            .with_context(|| format!("failed to read {}", stacks_dir.display()))?;
        let mut stacks = vec![];
        let mut load_issues = vec![];
        for entry in entries {
            load_catalog_entry(entry?, &mut stacks, &mut load_issues);
        }
        stacks.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(Self {
            stacks_dir: stacks_dir.to_path_buf(),
            stacks,
            load_issues,
        })
    }

    pub fn stacks(&self) -> &[StackConfig] {
        &self.stacks
    }

    pub fn into_stacks(self) -> Vec<StackConfig> {
        self.stacks
    }

    pub fn get(&self, name: &str) -> Option<&StackConfig> {
        self.stacks.iter().find(|stack| stack.name == name)
    }

    pub fn find_branch(&self, branch: &str) -> Option<&StackConfig> {
        self.stacks
            .iter()
            .find(|stack| stack.branch_index(branch).is_some())
    }

    pub fn save(&mut self, config: &StackConfig) -> Result<()> {
        config.validate()?;
        ensure_branch_ownership(&self.stacks, config)?;
        let path = self.stack_path(&config.name);
        state::save_stack(&path, config)?;
        if let Some(existing) = self
            .stacks
            .iter_mut()
            .find(|stack| stack.name == config.name)
        {
            *existing = config.clone();
            return Ok(());
        }
        self.stacks.push(config.clone());
        self.stacks
            .sort_by(|left, right| left.name.cmp(&right.name));
        Ok(())
    }

    pub fn delete(&mut self, name: &str) -> Result<()> {
        validate::validate_stack_name(name)?;
        let path = self.stack_path(name);
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
        self.stacks.retain(|stack| stack.name != name);
        Ok(())
    }

    pub fn rename_stack(&mut self, old: &str, new: &str) -> Result<()> {
        validate::validate_stack_name(new)?;
        if self.get(new).is_some() {
            bail!("Stack '{new}' already exists.");
        }
        let index = self
            .stacks
            .iter()
            .position(|stack| stack.name == old)
            .with_context(|| format!("Stack '{old}' does not exist."))?;
        let original = self.stacks.remove(index);
        let mut stack = original.clone();
        stack.name = new.to_string();
        if let Err(error) = self.save(&stack) {
            self.stacks.insert(index, original);
            return Err(error);
        }
        self.delete(old)
    }

    pub fn rename_branch(&mut self, old: &str, new: &str) -> Result<()> {
        let mut changed = vec![];
        for stack in &self.stacks {
            let mut updated = stack.clone();
            let mut changed_stack = false;
            if updated.base_branch == old {
                updated.base_branch = new.to_string();
                changed_stack = true;
            }
            for branch in &mut updated.branches {
                if branch.name == old {
                    branch.name = new.to_string();
                    changed_stack = true;
                }
            }
            if changed_stack {
                changed.push(updated);
            }
        }
        for stack in changed {
            let path = self.stack_path(&stack.name);
            state::save_stack(&path, &stack)?;
            if let Some(existing) = self.stacks.iter_mut().find(|item| item.name == stack.name) {
                *existing = stack;
            }
        }
        Ok(())
    }

    pub fn diagnose(&self, local_branches: &HashSet<String>) -> Vec<CatalogIssue> {
        let mut issues = self.integrity_issues();
        for stack in &self.stacks {
            if !local_branches.contains(&stack.base_branch) {
                issues.push(CatalogIssue {
                    message: format!(
                        "Stack '{}' has missing base branch '{}'.",
                        stack.name, stack.base_branch
                    ),
                    repairable: false,
                });
            }
            for branch in &stack.branches {
                if !local_branches.contains(&branch.name) {
                    issues.push(CatalogIssue {
                        message: format!(
                            "Stack '{}' tracks missing branch '{}'.",
                            stack.name, branch.name
                        ),
                        repairable: true,
                    });
                }
            }
        }
        issues
    }

    pub fn integrity_issues(&self) -> Vec<CatalogIssue> {
        let mut issues = self.load_issues.clone();
        issues.extend(find_duplicate_ownership(&self.stacks));
        issues
    }

    pub fn remove_missing_branches(&mut self, local_branches: &HashSet<String>) -> Result<usize> {
        let mut repaired = 0;
        let stacks = self.stacks.clone();
        for mut stack in stacks {
            let previous_len = stack.branches.len();
            stack
                .branches
                .retain(|branch| local_branches.contains(&branch.name));
            repaired += previous_len - stack.branches.len();
            if previous_len == stack.branches.len() {
                continue;
            }
            if stack.branches.is_empty() {
                self.delete(&stack.name)?;
                continue;
            }
            self.save(&stack)?;
        }
        Ok(repaired)
    }

    fn stack_path(&self, name: &str) -> PathBuf {
        self.stacks_dir.join(format!("{name}.toml"))
    }
}

fn load_catalog_entry(
    entry: std::fs::DirEntry,
    stacks: &mut Vec<StackConfig>,
    issues: &mut Vec<CatalogIssue>,
) {
    let path = entry.path();
    if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
        return;
    }
    match state::load_stack(&path) {
        Ok(stack) => {
            let file_name = path.file_stem().and_then(|name| name.to_str());
            if file_name != Some(&stack.name) {
                issues.push(CatalogIssue {
                    message: format!(
                        "Stack file '{}' declares stack name '{}'.",
                        path.display(),
                        stack.name
                    ),
                    repairable: false,
                });
            }
            stacks.push(stack);
        }
        Err(error) => issues.push(CatalogIssue {
            message: format!("Could not load '{}': {error}", path.display()),
            repairable: false,
        }),
    }
}

fn ensure_branch_ownership(stacks: &[StackConfig], candidate: &StackConfig) -> Result<()> {
    let owners = stacks
        .iter()
        .filter(|stack| stack.name != candidate.name)
        .flat_map(|stack| {
            stack
                .branches
                .iter()
                .map(move |branch| (branch.name.as_str(), stack.name.as_str()))
        })
        .collect::<HashMap<_, _>>();
    for branch in &candidate.branches {
        if let Some(owner) = owners.get(branch.name.as_str()) {
            bail!(
                "Branch '{}' is already tracked by stack '{}'.",
                branch.name,
                owner
            );
        }
    }
    Ok(())
}

fn find_duplicate_ownership(stacks: &[StackConfig]) -> Vec<CatalogIssue> {
    let mut owners: HashMap<&str, &str> = HashMap::new();
    let mut issues = vec![];
    for stack in stacks {
        for branch in &stack.branches {
            if let Some(owner) = owners.insert(&branch.name, &stack.name) {
                issues.push(CatalogIssue {
                    message: format!(
                        "Branch '{}' is tracked by both '{}' and '{}'.",
                        branch.name, owner, stack.name
                    ),
                    repairable: false,
                });
            }
        }
    }
    issues
}
