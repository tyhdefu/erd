# New idea #
Integrate with gitlab/github pipelines/artifacts in order to allow the use
- Scan repositories (under an organisation)
- Hash artifacts and match up if available. Flag same version different hash.
- Allow user to specify / correct URL of repository for a given artifact
  - Store this in a configuration file
- Allow the user to download the latest version of an artifact from a repository
  - Based on git history, but also provide version information
  - Show changes since last update
- Allow the user to rollback to a previous version if required
  - Allow the user to re-run pipelines (artifacts may have been cleaned up) to generate new versions
- Log actions so the user can easily revert
- Start with command line interface, maybe TUI in future.
- Runs on the system to deploy, and fetches directly from gitlab
  - How can we make this useful for the test server too?


## erd status ##
- List general information about the current directory
- Find outdated artifacts and suggest updating to newer ones

## erd update ##
- Update a given artifact
- Possibly allow building an older version too?

## erd add/remove ##
- Add / remove an entry in the config file that denotes the artifact name, gitlab repository it comes from

## erd rebuild ##
- Request that gitlab/github rebuilds a given artifact, so that it can be switched to later.

### Useful requests ###
Get details about the CBX project:   curl "https://gitlab.com/api/v4/projects/19575774"
Find branches, incl. default branch: curl "https://gitlab.com/api/v4/projects/19575774/repository/branches"

Download the latest artifact on the sponge branch
curl --location --output artifacts.zip "https://gitlab.com/api/v4/projects/19575774/jobs/artifacts/sponge/download?job=build"

Possible improvement to unzipping and searching:
https://docs.gitlab.com/ee/ci/jobs/job_artifacts.html#link-to-job-artifacts-in-the-merge-request-ui
