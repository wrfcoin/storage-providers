# Predicate for the Rust CI `gate` job (jq -e -f). `.` is the Actions "list jobs for a workflow run attempt" response.
# Exactly one job named "build", and it must be: conclusion success, at least one executed step, a runner_name that is a
# NON-EMPTY STRING on a job with the hosted ubuntu-latest label (null and "" are refused),
# and a successful "Workspace gate" step.
[.jobs[] | select(.name == "build")]
| length == 1
  and all(.[];
    .conclusion == "success"
    and ((.steps // []) | length) > 0
    and (.runner_name | type == "string" and length > 0)
    and (.labels | type == "array" and index("ubuntu-latest") != null)
    and any((.steps // [])[]; .name == "Workspace gate" and .conclusion == "success"))
