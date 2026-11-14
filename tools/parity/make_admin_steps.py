"""Generates tools/parity/admin-steps.jsonl: a scenario of team, project and membership management (creating, editing,
deleting, joining, leaving, inviting existing and new users, member settings, answering invitations, new users setting
their password), incl. what must be refused, plus the lists along the way. admin.sh runs it through upstream's views
and through our API. Steps name teams by their current name and projects by slug; users by email.

Each step: {"as": email, "op": ..., ...}; "modes" limits a step to "email" (invitations are emailed) or "links"."""
import json

ALICE, BOB, CAROL, ADMIN = "alice@example.org", "bob@example.org", "carol@example.org", "admin@example.org"
NEW, NEW2 = "new.person@example.org", "second.new@example.org"


def step(as_, op, **kw):
    return {"as": as_, "op": op, **kw}


PROJECT = {"visibility": 99, "retention_max_event_count": 5000, "grouping_mechanism": "bugsink-v2"}

MESSAGING = [
    # messaging services ("@CAPTURE@": the capture server's address as each side sees it)
    step(BOB, "service_add", project="web-app", display_name="Slack", kind="slack",
         config={"webhook_url": "http://@CAPTURE@/slack"}),  # not an admin
    step(ALICE, "service_add", project="web-app", display_name="Slack", kind="slack",
         config={"webhook_url": "http://@CAPTURE@/slack"}),
    step(ALICE, "service_add", project="web-app", display_name="  ", kind="discord",
         config={"webhook_url": "not a url"}),  # both forms' errors
    step(ALICE, "service_add", project="web-app", display_name="Discord", kind="discord",
         config={"webhook_url": "//@CAPTURE@/discord"}),  # https assumed
    step(ALICE, "service_add", project="web-app", display_name="Nowhere", kind="google_chat",
         config={"webhook_url": "http://no-such-host.invalid/x"}),
    step(ALICE, "service_add", project="web-app", display_name="Teams", kind="msteams",
         config={"webhook_url": "http://@CAPTURE@/msteams?" + "x" * 300}),  # long URLs are fine for Teams
    step(ALICE, "service_add", project="web-app", display_name="Long", kind="slack",
         config={"webhook_url": "http://@CAPTURE@/slack?" + "x" * 300}),
    step(ALICE, "service_add", project="web-app", display_name="Mattermost", kind="mattermost",
         config={"webhook_url": "http://@CAPTURE@/mattermost", "channel": " town-square "}),
    step(ALICE, "service_add", project="web-app", display_name="Google", kind="google_chat",
         config={"webhook_url": "http://@CAPTURE@/google_chat"}),
    step(ALICE, "service_add", project="web-app", display_name="Custom", kind="custom",
         config={"webhook_url": "http://@CAPTURE@/fail/custom"}),
    step(ALICE, "service_add", project="web-app", display_name="Telegram", kind="telegram",
         config={"bot_token": "123:abc", "chat_id": "@channel", "message_thread_id": "5"}),  # topics: numeric ids
    step(ALICE, "service_add", project="web-app", display_name="Telegram", kind="telegram",
         config={"bot_token": "", "chat_id": "chan", "message_thread_id": "0"}),
    step(ALICE, "service_add", project="web-app", display_name="Telegram", kind="telegram",
         config={"bot_token": "123:abc", "chat_id": "-100123", "message_thread_id": "7"}),
    step(ALICE, "services", project="web-app"),
    step(ALICE, "service_edit", project="web-app", service="Discord", display_name="Discord <ops> & *dev*",
         config={"webhook_url": "http://@CAPTURE@/discord"}),
    step(ALICE, "service_edit", project="web-app", service="Mattermost", display_name="",
         config={"webhook_url": "gopher://@CAPTURE@/x", "channel": "x"}),
    step(BOB, "service_test", project="web-app", service="Slack"),  # not an admin
    *[step(ALICE, "service_test", project="web-app", service=name)
      for name in ["Slack", "Discord <ops> & *dev*", "Teams", "Mattermost", "Google", "Custom"]],
    step(ALICE, "service_remove", project="web-app", service="Google"),
    step(ALICE, "services", project="web-app"),
]

DORA = "dora@example.org"
ACCOUNTS = [
    # signing up (email mode: the address is confirmed by email first; links mode: USER_REGISTRATION_VERIFY_EMAIL off)
    step(DORA, "signup", email="not-an-email", password="lovelace-quantum-77"),
    step(DORA, "signup", email=ALICE, password="lovelace-quantum-77"),  # taken
    step(DORA, "signup", email="a!b@example.org", password="lovelace-quantum-77"),  # not a valid username
    step(DORA, "signup", email=DORA, password="12345678"),  # common, numeric
    step(DORA, "signup", email=DORA, password="dora.example"),  # too similar to the username
    step(DORA, "signup", email=" " + DORA + " ", password="lovelace-quantum-77"),
    step(DORA, "signup", email=DORA, password="lovelace-quantum-77"),  # email mode: waiting for confirmation
    step(DORA, "resend_confirmation", email=DORA, modes=["email"]),
    step(DORA, "resend_confirmation", email=ALICE),  # already confirmed
    step(DORA, "resend_confirmation", email="nobody@example.org"),
    step(DORA, "confirm_email", token_for=DORA, modes=["email"]),
    step(DORA, "team_list", tab="other"),  # logged in either way
    # resetting a password, preferences, changing a password
    step(BOB, "request_reset_password", email="nobody@example.org"),
    step(BOB, "request_reset_password", email="not an email"),
    step(BOB, "request_reset_password", email=BOB),
    step(BOB, "reset_password", token_for=BOB, password="Bob-has-a-new-1"),
    step(BOB, "preferences", send_email_alerts=False, theme_preference="neon", language="en"),
    step(BOB, "preferences", send_email_alerts=False, theme_preference="dark", language="zh-hans"),
    step(CAROL, "change_password", old_password="wrong", new_password="lovelace-quantum-77"),
    step(CAROL, "change_password", old_password="pw-carol", new_password="carol"),
    step(CAROL, "change_password", old_password="pw-carol", new_password="lovelace-quantum-77"),
    step(CAROL, "team_list", tab="mine"),  # this session still works
]

steps = [
    *ACCOUNTS,
    step(BOB, "team_list", tab="mine"),
    step(ALICE, "team_create", name="Alpha", visibility=10),
    step(ALICE, "team_create", name="Alpha", visibility=10),  # taken
    step(ALICE, "team_create", name="  ", visibility=5),  # required; not a choice
    step(BOB, "team_list", tab="other"),
    step(BOB, "team_join", team="Alpha"),  # discoverable, not joinable
    step(ALICE, "team_edit", team="Alpha", name="Alpha Team", visibility=1),
    step(BOB, "team_edit", team="Alpha Team", name="Mine Now", visibility=1),  # not an admin
    step(BOB, "team_join", team="Alpha Team"),
    step(BOB, "team_join", team="Alpha Team"),  # already a member
    step(BOB, "team_invite", team="Alpha Team", email=CAROL, role=0),  # not an admin
    step(ALICE, "team_invite", team="Alpha Team", email=CAROL, role=0),
    step(ALICE, "team_invite", team="Alpha Team", email=NEW, role=1),
    step(ALICE, "team_invite", team="Alpha Team", email="not-an-email", role=0),
    step(ALICE, "team_invite", team="Alpha Team", email=" " + NEW2 + " ", role=7),
    step(ALICE, "team_members", team="Alpha Team"),
    step(ALICE, "team_reinvite", team="Alpha Team", user=CAROL, modes=["email"]),
    step(ALICE, "team_invite_link", team="Alpha Team", user=CAROL, modes=["links"]),
    step(ALICE, "team_invite_link", team="Alpha Team", user=NEW, modes=["links"]),  # can't set a password this way
    step(CAROL, "team_list", tab="mine"),
    step(CAROL, "team_member_settings", team="Alpha Team", user=CAROL, role=1, send_email_alerts=False),  # invited
    step(CAROL, "team_accept", team="Alpha Team"),
    step(CAROL, "team_member_settings", team="Alpha Team", user=CAROL, role=1, send_email_alerts=False),  # no role
    step(ALICE, "team_member_settings", team="Alpha Team", user=BOB, role=1),  # upstream: needs superuser *and* admin
    step(ADMIN, "team_member_settings", team="Alpha Team", user=BOB, role=1),  # superuser but not a member
    step(ALICE, "team_member_settings", team="Alpha Team", user=ALICE, role=1, send_email_alerts=None),
    step(ALICE, "team_leave", team="Alpha Team"),  # the last admin
    step(BOB, "team_leave", team="Alpha Team"),
    step(BOB, "team_join", team="Alpha Team"),
    step(NEW, "accept_new_user", group="team", team="Alpha Team", token_for=NEW),
    step(NEW, "reset_password", token_for=NEW, password="password"),  # too common
    step(NEW, "reset_password", token_for=NEW, password="new.person1"),  # too similar to the username
    step(NEW, "reset_password", token_for=NEW, password="lovelace-quantum-77"),
    step(NEW, "team_accept", team="Alpha Team"),
    step(NEW, "team_list", tab="mine"),

    step(CAROL, "project_create", team="Alpha Team", name="Web App", **PROJECT),  # not a team admin
    step(ALICE, "project_create", team="Alpha Team", name="Web App", **PROJECT),
    step(ALICE, "project_create", team="Alpha Team", name="Web App", **PROJECT),  # taken in this team
    step(ALICE, "project_create", team="Alpha Team", name="web app!!", **PROJECT),  # slug taken: web-app-0
    step(ALICE, "project_create", team="Alpha Team", name="Héllo Wörld", visibility=1,
         retention_max_event_count=-1, grouping_mechanism="nope"),
    step(ALICE, "project_create", team="Alpha Team", name="Héllo Wörld", visibility=1,
         retention_max_event_count=100, grouping_mechanism="bugsink-v1"),
    step(ADMIN, "team_create", name="Beta", visibility=99),
    step(ALICE, "project_create", team="Beta", name="Elsewhere", **PROJECT),  # not her team
    step(ADMIN, "project_create", team="Beta", name="Elsewhere", **PROJECT),
    step(BOB, "project_list", tab="teams"),
    step(BOB, "project_join", project="web-app"),  # through the team
    step(BOB, "project_join", project="web-app"),  # already
    step(BOB, "project_join", project="elsewhere"),  # not joinable
    step(CAROL, "project_join", project="web-app-0"),
    step(ALICE, "project_invite", project="web-app", email=BOB, role=0),  # already a member: unchanged
    step(ALICE, "project_invite", project="web-app", email=NEW2, role=0),
    step(ALICE, "project_invite", project="web-app", email=CAROL, role=1),
    step(ALICE, "project_members", project="web-app"),
    step(ALICE, "project_member_settings", project="web-app", user=BOB, role=1),
    step(BOB, "project_member_settings", project="web-app", user=BOB, role=0, send_email_alerts=True),
    step(BOB, "project_remove", project="web-app-0", user=CAROL),
    step(CAROL, "project_decline", project="web-app"),
    step(CAROL, "project_accept", project="web-app"),  # declined: nothing to accept
    step(NEW2, "accept_new_user", group="project", project="web-app", token_for=NEW2),
    step(NEW2, "reset_password", token_for=NEW2, password="Kx9!plum-orchard"),
    step(NEW2, "project_accept", project="web-app"),
    step(NEW2, "project_accept", project="web-app"),  # already accepted
    step(NEW2, "project_member_settings", project="web-app", user=NEW2, role=1, send_email_alerts=False),
    *MESSAGING,
    step(BOB, "project_list", tab="mine"),
    step(CAROL, "project_list", tab="other"),
    step(BOB, "sdk_setup", project="web-app"),
    step(CAROL, "sdk_setup", project="elsewhere"),  # no access
    step(ALICE, "project_edit", project="web-app", name="Web Application", visibility=10,
         retention_max_event_count=100, grouping_mechanism="bugsink-v1"),
    step(BOB, "project_edit", project="elsewhere", name="Mine", **PROJECT),  # not an admin
    step(ALICE, "project_edit", project="web-app-0", name="Web Application", **PROJECT),  # taken in the team
    step(ALICE, "project_leave", project="web-app"),
    step(ALICE, "project_delete", project="web-app-0"),
    step(ALICE, "project_edit", project="web-app-0", name="Gone", **PROJECT),  # deleted
    step(ALICE, "team_delete", team="Alpha Team"),  # its projects have members: refused
    step(ADMIN, "project_delete", project="elsewhere"),
    step(ADMIN, "team_delete", team="Beta"),  # its only project was deleted (in the background): fine
    step(ADMIN, "team_create", name="Gamma", visibility=99),
    step(ADMIN, "team_delete", team="Gamma"),
    step(CAROL, "team_list", tab="other"),
    step(ALICE, "team_remove", team="Alpha Team", user=BOB),
    step(ALICE, "team_remove", team="Alpha Team", user=ALICE),  # removing yourself: leaving (last admin)
]

ALL_CAPABILITIES = ["issues:read", "events:read", "issues:comment", "issues:triage", "issues:delete", "releases:read",
                    "releases:create", "debug-files:upload", "projects:read", "projects:manage", "teams:read",
                    "teams:manage"]
INSTALLATION = [
    step(DORA, "home"),
    step(BOB, "home"),
    step(ALICE, "home"),
    step(ALICE, "users"),  # superusers only
    step(ADMIN, "users"),
    step(ADMIN, "user_edit", user=DORA, email="not-an-email"),
    step(ADMIN, "user_edit", user=DORA, email=ALICE),  # taken
    step(ADMIN, "user_edit", user=DORA, email="a!b@example.org"),  # not a valid username
    step(ADMIN, "user_edit", user=DORA, email=" dora.new@example.org "),
    step(ADMIN, "user_delete", user=ALICE),  # active: refused
    step(ADMIN, "user_deactivate", user=NEW2),
    step(ADMIN, "user_activate", user=NEW2),
    step(ADMIN, "user_deactivate", user=CAROL),
    step(ADMIN, "user_delete", user=CAROL),  # memberships go, comments and tokens lose their user
    step(ALICE, "token_create", description="nothing", capabilities=[]),
    step(ALICE, "token_create", description="", project="web-app", capabilities=["issues:read", "teams:read"]),
    step(ALICE, "token_create", description="CI", project="web-app", capabilities=["issues:read", "events:read"]),
    step(ALICE, "token_create", description="mine anyway", is_user_bound=False, capabilities=["projects:read"]),
    step(ALICE, "token_create", description="theirs", project="elsewhere", capabilities=["issues:read"]),  # deleted
    step(ADMIN, "token_create", description="service", is_user_bound=False, capabilities=ALL_CAPABILITIES),
    step(ADMIN, "token_create", description="personal", is_user_bound=True, capabilities=["teams:manage"]),
    step(ALICE, "token_list"),
    step(ADMIN, "token_list"),
    step(ALICE, "token_revoke", token="service"),  # not hers
    step(ALICE, "token_revoke", token="CI"),
    step(ALICE, "token_revoke", token="CI"),  # already
    step(ADMIN, "token_list"),
    step(ALICE, "counts"),
    step(ADMIN, "counts"),
    step(ALICE, "settings"),
    step(ADMIN, "settings"),
    step(ALICE, "system_warnings"),
    step(ADMIN, "system_warnings"),
    step(ALICE, "silence_email"),
    step(ADMIN, "silence_email"),
    step(ALICE, "system_warnings"),
]
steps += INSTALLATION

with open("tools/parity/admin-steps.jsonl", "w") as f:
    for i, s in enumerate(steps):
        f.write(json.dumps({"n": i, **s}) + "\n")
print(len(steps), "steps")
