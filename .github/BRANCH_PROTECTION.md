# Branch Protection Rules Setup Guide

This document explains how to set up branch protection rules to prevent direct pushes to the `main` branch and require PRs with CI checks.

## Setup Steps

### 1. Open Repository Settings

1. Go to the GitHub repository page
2. Click the **Settings** tab
3. Select **Branches** from the left menu

### 2. Add Branch Protection Rule

1. Click **Add branch protection rule** or **Add rule**
2. Enter `main` in the **Branch name pattern** field

### 3. Configure Protection Rules

Check the following options:

#### Required Settings

- [x] **Require a pull request before merging**
  - Prevents direct pushes to main branch, requires PR
  - Sub-options:
    - [x] **Require approvals** (optional): Require review approval
    - Number of approvals: 1 (recommended)

- [x] **Require status checks to pass before merging**
  - Prevents merging when CI fails
  - [x] **Require branches to be up to date before merging**
  - Add the following to **Status checks that are required**:
    - `test (macos-latest)`
    - `test (windows-latest)`

#### Recommended Settings

- [x] **Do not allow bypassing the above settings**
  - Apply rules to administrators as well

- [x] **Require conversation resolution before merging** (optional)
  - Prevent merging until all comments are resolved

### 4. Save the Rule

Click **Create** or **Save changes** to save

## Behavior After Setup

| Action | Allowed |
|--------|---------|
| Direct push to main branch | Blocked |
| Create PR | Allowed |
| Merge with failing CI | Blocked |
| Merge with passing CI | Allowed |

## About CI Status Checks

This repository runs the following checks via `.github/workflows/ci.yml`:

- TypeScript type checking
- Rust compile checking
- Tauri app build (macOS, Windows)

PRs cannot be merged until all checks pass.

## Reference Links

- [GitHub Docs: Managing a branch protection rule](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/managing-a-branch-protection-rule)
