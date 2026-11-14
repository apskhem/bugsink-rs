-- The management state of a Bugsink database, comparable across databases: natural keys instead of random ids, and
-- what a login changes (last_login, rehashed passwords) left out. One row per line, fields separated by " | ".
SELECT concat_ws(' | ', 'user', email, username, is_active, is_superuser, first_name, last_name,
                 password <> '' AND password NOT LIKE '!%', send_email_alerts, theme_preference, language)
FROM auth_user ORDER BY email;
SELECT concat_ws(' | ', 'team', name, visibility) FROM teams_team ORDER BY name;
SELECT concat_ws(' | ', 'team_member', t.name, u.email, m.role, m.accepted, coalesce(m.send_email_alerts::text, 'null'))
FROM teams_teammembership m JOIN teams_team t ON t.id = m.team_id JOIN auth_user u ON u.id = m.user_id
ORDER BY t.name, u.email;
SELECT concat_ws(' | ', 'project', p.slug, p.name, coalesce(t.name, 'null'), p.visibility, p.retention_max_event_count,
                 p.grouping_mechanism, coalesce(p.previous_grouping_mechanism, 'null'),
                 p.grouping_mechanism_upgraded_at IS NOT NULL, p.is_deleted, p.alert_on_new_issue,
                 p.alert_on_regression, p.alert_on_unmute, p.has_releases, p.issue_count, p.digested_event_count,
                 p.stored_event_count, p.quota_exceeded_reason, p.next_quota_check, p.quota_exceeded_until IS NULL)
FROM projects_project p LEFT JOIN teams_team t ON t.id = p.team_id ORDER BY p.slug;
SELECT concat_ws(' | ', 'project_member', p.slug, u.email, m.role, m.accepted, coalesce(m.send_email_alerts::text, 'null'))
FROM projects_projectmembership m JOIN projects_project p ON p.id = m.project_id JOIN auth_user u ON u.id = m.user_id
ORDER BY p.slug, u.email;
SELECT concat_ws(' | ', 'token', t.description, coalesce(u.email, 'null'), coalesce(p.slug, 'null'), t.is_user_bound,
                 t.is_project_bound, t.expires_at IS NOT NULL, t.issues_read, t.events_read, t.issues_comment,
                 t.issues_triage, t.issues_delete, t.releases_read, t.releases_create, t.debug_files_upload,
                 t.projects_read, t.projects_manage, t.teams_read, t.teams_manage, length(t.token))
FROM bsmain_authtoken t LEFT JOIN auth_user u ON u.id = t.user_id LEFT JOIN projects_project p ON p.id = t.project_id
ORDER BY t.description, t.id;
SELECT concat_ws(' | ', 'installation', silence_email_system_warning) FROM phonehome_installation;
SELECT concat_ws(' | ', 'verification', u.email, v.email, count(*))
FROM users_emailverification v JOIN auth_user u ON u.id = v.user_id GROUP BY u.email, v.email ORDER BY u.email;
SELECT concat_ws(' | ', 'service', p.slug, s.display_name, s.kind, s.config, s.last_failure_timestamp IS NOT NULL,
                 coalesce(s.last_failure_status_code::text, 'null'), coalesce(s.last_failure_response_text, 'null'),
                 coalesce(s.last_failure_is_json::text, 'null'), coalesce(s.last_failure_error_type, 'null'),
                 coalesce(s.last_failure_error_message, 'null'))
FROM alerts_messagingserviceconfig s JOIN projects_project p ON p.id = s.project_id ORDER BY p.slug, s.display_name, s.id;
