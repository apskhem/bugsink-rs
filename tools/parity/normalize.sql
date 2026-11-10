-- Dumps the digest-relevant state of a Bugsink database in a form that is comparable across databases: random ids
-- are replaced by natural keys, and the random retention irrelevance is left out.
\pset footer off
SELECT 'project', id, digested_event_count, stored_event_count, issue_count, has_releases, next_quota_check,
       quota_exceeded_until, quota_exceeded_reason
FROM projects_project ORDER BY id;
SELECT 'installation', next_quota_check, quota_exceeded_until, quota_exceeded_reason,
       regexp_replace(email_quota_usage, '"[0-9T-]+"', '"<period>"', 'g'),
       (SELECT string_agg(a->>'ok', ',') FROM json_array_elements(email_sending_diagnostics::json->'attempts') a)
FROM phonehome_installation;
-- The error message names the address that was posted to, which differs per side.
SELECT 'messagingservice', id, kind, last_failure_timestamp IS NOT NULL, last_failure_error_type,
       last_failure_status_code, last_failure_response_text, last_failure_is_json
FROM alerts_messagingserviceconfig ORDER BY id;
SELECT 'issue', project_id, digest_order, first_seen, last_seen, digested_event_count, stored_event_count,
       calculated_type, calculated_value, transaction, last_frame_filename, last_frame_module, last_frame_function,
       is_resolved, is_resolved_by_next_release, is_resolved_unconditionally, replace(fixed_at, E'\n', '|'),
       replace(events_at, E'\n', '|'), is_muted, unmute_on_volume_based_conditions, unmute_after, next_unmute_check,
       is_deleted
FROM issues_issue ORDER BY project_id, digest_order;
SELECT 'grouping', g.project_id, i.digest_order, g.grouping_key, g.grouping_key_hash, g.grouping_mechanism
FROM issues_grouping g JOIN issues_issue i ON i.id = g.issue_id ORDER BY 2, 3, 6, 4;
SELECT 'event', e.project_id, e.project_digest_order, e.event_id, i.digest_order, g.grouping_key, g.grouping_mechanism,
       e.ingested_at, e.digested_at, md5(e.data), e.storage_backend, e.timestamp, e.platform, e.level, e.logger,
       e.transaction, e.server_name, e.release, e.dist, e.environment, e.sdk_name, e.sdk_version, e.calculated_type,
       e.calculated_value, e.last_frame_filename, e.last_frame_module, e.last_frame_function, e.remote_addr,
       e.digest_order, e.never_evict
FROM events_event e JOIN issues_issue i ON i.id = e.issue_id JOIN issues_grouping g ON g.id = e.grouping_id
ORDER BY e.project_id, e.project_digest_order;
SELECT 'turningpoint', t.project_id, i.digest_order, e.event_id, t.timestamp, t.kind, t.metadata, t.comment, t.user_id
FROM issues_turningpoint t JOIN issues_issue i ON i.id = t.issue_id LEFT JOIN events_event e ON e.id = t.triggering_event_id
ORDER BY 2, 3, 5, 6, 4;
SELECT 'release', project_id, version, date_released, semver, is_semver, sort_epoch FROM releases_release ORDER BY 2, 3;
SELECT 'count_installation', bucket, count FROM events_installationeventcountsperhour ORDER BY 2;
SELECT 'count_project', project_id, bucket, count FROM events_projecteventcountsperhour ORDER BY 2, 3;
SELECT 'count_issue', c.project_id, i.digest_order, c.bucket, c.count, c.digest_order
FROM events_issueeventcountsperhour c JOIN issues_issue i ON i.id = c.issue_id ORDER BY 2, 3, 4;
SELECT 'tagkey', project_id, key, mostly_unique FROM tags_tagkey ORDER BY 2, 3;
SELECT 'tagvalue', v.project_id, k.key, v.value FROM tags_tagvalue v JOIN tags_tagkey k ON k.id = v.key_id ORDER BY 2, 3, 4;
SELECT 'eventtag', et.project_id, e.event_id, i.digest_order, k.key, v.value, et.digest_order
FROM tags_eventtag et JOIN events_event e ON e.id = et.event_id JOIN issues_issue i ON i.id = et.issue_id
JOIN tags_tagvalue v ON v.id = et.value_id JOIN tags_tagkey k ON k.id = v.key_id ORDER BY 2, 3, 5;
SELECT 'issuetag', it.project_id, i.digest_order, k.key, v.value, it.count, it.key_id = v.key_id
FROM tags_issuetag it JOIN issues_issue i ON i.id = it.issue_id JOIN tags_tagvalue v ON v.id = it.value_id
JOIN tags_tagkey k ON k.id = it.key_id ORDER BY 2, 3, 4, 5;
