use anyhow::{bail, Result};

/// Validate a stack name for filesystem safety and git compatibility.
/// Stack names are used to construct file paths (.git/gw/stacks/<name>.toml),
/// so they must not contain path traversal sequences or filesystem-unsafe characters.
pub fn validate_stack_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Stack name cannot be empty");
    }

    if name.starts_with('-') {
        bail!("Stack name cannot start with '-'");
    }

    if name.contains("..") {
        bail!("Stack name cannot contain '..'");
    }

    if name.contains('/') || name.contains('\\') {
        bail!("Stack name cannot contain path separators ('/' or '\\')");
    }

    if name.contains('\0') {
        bail!("Stack name cannot contain null bytes");
    }

    // Only allow alphanumeric, hyphens, underscores, and dots
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        bail!("Stack name can only contain alphanumeric characters, hyphens, underscores, and dots");
    }

    Ok(())
}

/// Validate a branch name for git safety.
/// Branch names are passed as arguments to git commands, so they must not
/// be interpretable as flags or contain git-unsafe characters.
pub fn validate_branch_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Branch name cannot be empty");
    }

    if name.starts_with('-') {
        bail!("Branch name cannot start with '-' (would be interpreted as a git flag)");
    }

    if name.contains("..") {
        bail!("Branch name cannot contain '..' (git ref traversal)");
    }

    if name.contains('\0') {
        bail!("Branch name cannot contain null bytes");
    }

    if name.ends_with(".lock") {
        bail!("Branch name cannot end with '.lock'");
    }

    if name.ends_with('/') {
        bail!("Branch name cannot end with '/'");
    }

    // Check for git-unsafe characters
    for c in ['~', '^', ':', '?', '*', '[', '\\'] {
        if name.contains(c) {
            bail!("Branch name cannot contain '{c}'");
        }
    }

    // Check for control characters
    if name.chars().any(|c| c.is_control()) {
        bail!("Branch name cannot contain control characters");
    }

    // Check for spaces
    if name.contains(' ') {
        bail!("Branch name cannot contain spaces");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_stack_names() {
        assert!(validate_stack_name("auth").is_ok());
        assert!(validate_stack_name("my-feature").is_ok());
        assert!(validate_stack_name("feature_123").is_ok());
        assert!(validate_stack_name("v1.0").is_ok());
    }

    #[test]
    fn invalid_stack_names() {
        assert!(validate_stack_name("").is_err());
        assert!(validate_stack_name("-bad").is_err());
        assert!(validate_stack_name("../../etc").is_err());
        assert!(validate_stack_name("path/traversal").is_err());
        assert!(validate_stack_name("back\\slash").is_err());
        assert!(validate_stack_name("has spaces").is_err());
    }

    #[test]
    fn valid_branch_names() {
        assert!(validate_branch_name("feature/auth").is_ok());
        assert!(validate_branch_name("fix-bug-123").is_ok());
        assert!(validate_branch_name("my_branch").is_ok());
    }

    #[test]
    fn invalid_branch_names() {
        assert!(validate_branch_name("").is_err());
        assert!(validate_branch_name("--force").is_err());
        assert!(validate_branch_name("bad..ref").is_err());
        assert!(validate_branch_name("has space").is_err());
        assert!(validate_branch_name("file.lock").is_err());
        assert!(validate_branch_name("trail/").is_err());
    }
}
