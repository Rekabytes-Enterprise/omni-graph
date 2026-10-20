# Maintainers and project decisions

## Ownership

[@rekabytes](https://github.com/rekabytes) is the current review and release owner.
[CODEOWNERS](.github/CODEOWNERS) requests review for repository changes.
It does not grant repository access or enforce approvals by itself.

Contributors are welcome to help with bug triage, documentation, tests, agent
integrations, accessibility, and platform support. Maintainers decide what merges
and ships, considering user needs, compatibility, data safety, and maintenance
cost. For substantial changes, open an issue with the problem, proposed behavior,
alternatives, and compatibility implications before implementing it. An issue is
a proposal, not a delivery commitment.

## Review and release checklist

1. Confirm contribution branches start from `dev` and PRs target `dev`.
2. Review the implementation, relevant checks, documentation, compatibility, and
   handling of private memory data. Run untrusted contributions in an isolated
   environment without credentials or real memories.
3. Wait for the required verification checks before merging. Never execute
   contributor code with repository secrets to work around fork restrictions.
4. For a release, follow [the release process](docs/development/releases.md).
   Promote the verified commit to `main`, use the matching version tag, and verify
   published installation assets. Do not move existing release tags.
5. Review `Closes #…` references from included contribution PRs. When a release
   resolves an issue, close it with a link to the release and PR if GitHub has not
   already closed it. Keep partially addressed issues open.

## Repository settings checklist

These settings require a repository administrator; the files in this repository
do not configure them automatically:

- Enable **private vulnerability reporting** under Settings → Code security so
  the reporting form linked in [SECURITY.md](SECURITY.md) works.
- Protect `dev` and `main` with rulesets appropriate to the release process.
  Require PR review and the Verify checks on `dev`; verification runs on `dev`,
  not on `main`. Require code-owner review when another eligible reviewer is
  available. Preserve a controlled path for verified release promotion to `main`.
- Review organization/repository access and recovery options. Keep CODEOWNERS
  aligned with people who have write permission.
- Review dependency alerts and secret scanning options available to the repository.

No branch protection, access grant, or hosted reporting setting is implied by
these recommendations. Maintainer additions require agreement from the existing
maintainer and the new owner, followed by updates to this file and CODEOWNERS.
