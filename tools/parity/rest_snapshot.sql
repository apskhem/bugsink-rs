-- What rest_api.sh compares in the database after the requests (timestamps of the run and new ids are normalized).
SELECT concat_ws(' | ', 'issue', p.slug || '-' || i.digest_order, i.is_resolved, i.is_resolved_unconditionally,
                 i.is_resolved_by_next_release, replace(i.fixed_at, E'\n', ';'), i.is_muted,
                 i.unmute_on_volume_based_conditions, i.unmute_after, i.next_unmute_check, i.is_deleted,
                 i.digested_event_count, i.stored_event_count)
FROM issues_issue i JOIN projects_project p ON p.id = i.project_id ORDER BY p.slug, i.digest_order;
SELECT concat_ws(' | ', 'turningpoint', p.slug || '-' || i.digest_order, tp.kind, coalesce(tp.user_id::text, 'null'),
                 tp.comment, tp.metadata, tp.timestamp, coalesce(tp.triggering_event_id::text, 'null'))
FROM issues_turningpoint tp JOIN issues_issue i ON i.id = tp.issue_id JOIN projects_project p ON p.id = i.project_id
ORDER BY p.slug, i.digest_order, tp.timestamp, tp.kind, tp.comment;
SELECT concat_ws(' | ', 'project', id, coalesce(team_id::text, 'null'), name, slug, is_deleted, has_releases,
                 issue_count, digested_event_count, stored_event_count, alert_on_new_issue, alert_on_regression,
                 alert_on_unmute, visibility, quota_exceeded_reason, next_quota_check, retention_max_event_count,
                 grouping_mechanism, coalesce(previous_grouping_mechanism, 'null'),
                 coalesce(grouping_mechanism_upgraded_at::text, 'null'))
FROM projects_project ORDER BY id;
SELECT concat_ws(' | ', 'team', id, name, visibility) FROM teams_team ORDER BY name;
SELECT concat_ws(' | ', 'release', project_id, version, date_released, semver, is_semver, sort_epoch)
FROM releases_release ORDER BY project_id, version;
SELECT concat_ws(' | ', 'membership', project_id, user_id, role, accepted) FROM projects_projectmembership
ORDER BY project_id, user_id;
SELECT concat_ws(' | ', 'teammembership', team_id, user_id, role, accepted) FROM teams_teammembership
ORDER BY team_id, user_id;
SELECT concat_ws(' | ', 'count', 'events', count(*)) FROM events_event;
SELECT concat_ws(' | ', 'count', 'eventtags', count(*)) FROM tags_eventtag;
SELECT concat_ws(' | ', 'count', 'issuetags', count(*)) FROM tags_issuetag;
SELECT concat_ws(' | ', 'count', 'groupings', count(*)) FROM issues_grouping;
SELECT concat_ws(' | ', 'count', 'issue_counts_per_hour', count(*)) FROM events_issueeventcountsperhour;
