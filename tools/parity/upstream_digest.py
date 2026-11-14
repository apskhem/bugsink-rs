# Runs inside the official Bugsink image (`bugsink-manage shell`): replays digest steps (JSONL on stdin) through
# upstream's own digest_event, issue actions the way upstream's views apply them, and vacuuming.
import json
import sys
from datetime import datetime
from unittest import mock

from django.db import connection, transaction
from django.utils import timezone

from ingest.tasks import digest_event
from issues.models import (
    Issue, IssueQuerysetStateManager, IssueStateManager, apply_issue_action, is_valid_issue_action,
    q_for_invalid_issue_action)
from issues.tasks import delete_marked_issues_sync
from tags.tasks import vacuum_eventless_issuetags_sync, vacuum_tags_sync
from users.models import User
from snappea.settings import get_settings as get_snappea_settings

# Run on-commit tasks (zero-retention deletion) inline, as replay.rs does after each step.
get_snappea_settings()["TASK_ALWAYS_EAGER"] = True

for line in sys.stdin:
    step = json.loads(line)
    if "sql" in step:
        with connection.cursor() as cursor:
            cursor.execute(step["sql"])
        continue
    if "vacuum" in step:  # `bugsink-manage vacuum --deleted-issues --eventless-issuetags --tags`, in its order
        delete_marked_issues_sync()
        vacuum_eventless_issuetags_sync()
        vacuum_tags_sync()
        continue
    if "action" in step:
        with connection.cursor() as cursor:
            cursor.execute(step["issues"])
            issue_ids = [row[0] for row in cursor.fetchall()]
        action, user = step["action"], User.objects.get(pk=step["user_id"])
        # a view's transaction: on-commit tasks (delete_issue_deps) run after it, as in production
        with mock.patch.object(timezone, "now", return_value=datetime.fromisoformat(step["now"])), transaction.atomic():
            if step["bulk"]:  # _issue_list_pt_1
                issue_qs = Issue.objects.filter(project_id=step["project_id"], is_deleted=False, pk__in=issue_ids)
                illegal_conditions = q_for_invalid_issue_action(action)
                list(issue_qs.filter(illegal_conditions).values_list("id", flat=True))
                apply_issue_action(IssueQuerysetStateManager, issue_qs.exclude(illegal_conditions), action, user)
            else:  # issue_membership_required + _handle_post, per issue
                for issue_id in issue_ids:
                    issue = Issue.objects.filter(pk=issue_id, is_deleted=False).first()
                    if issue is not None and is_valid_issue_action(action, issue):
                        apply_issue_action(IssueStateManager, issue, action, user)
                        issue.save()
        continue
    metadata = {k: step[k] for k in ["event_id", "project_id", "ingested_at", "remote_addr"]}
    try:
        digest_event(metadata, step["event"], digested_at=datetime.fromisoformat(step["digested_at"]))
    except Exception as e:  # noqa
        print(f"step {step['n']}: {type(e).__name__}: {e}")
