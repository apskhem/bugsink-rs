# Runs inside upstream's `bugsink-manage shell` for admin.sh: the steps (JSONL on stdin) through upstream's own views
# (form posts with Django's RequestFactory; `render` captured), one JSON line per step: the outcome (ok, invalid with
# the form's errors, forbidden, not_found, refused), an invite link if one was shown, what a list showed, and the
# database snapshot (admin_snapshot.sql) afterwards. MODE (email/links) skips the other mode's steps.
import json
import os
import re
import sys
import uuid
from unittest import mock

from django.contrib.auth.models import AnonymousUser
from django.contrib.sessions.backends.db import SessionStore
from django.core.exceptions import ObjectDoesNotExist, PermissionDenied
from django.db import IntegrityError, connection
from django.http import Http404, HttpResponse
from django.test import RequestFactory
from django.utils import translation

import django.contrib.messages.api
import projects.views as pv
import teams.views as tv
import users.views as uv
from projects.models import Project
from snappea.settings import get_settings as get_snappea_settings
from alerts.models import MessagingServiceConfig
from teams.models import Team
from users.models import EmailVerification, User

get_snappea_settings()["TASK_ALWAYS_EAGER"] = True  # invitation emails and project deletion run inline
MODE = os.environ["MODE"]
CAPTURE = "captu.test:8025"
SNAPSHOT = [s for s in open("/out/admin_snapshot.sql").read().split(";")
            if s.strip() and not all(line.startswith("--") or not line.strip() for line in s.splitlines())]
rf = RequestFactory(HTTP_HOST=os.environ["HOST"])
captured, levels = {}, []


def capture(request, template, context=None, *args, **kwargs):
    captured.clear()
    captured.update(context or {})
    return HttpResponse()


def record_message(request, level, message, *args, **kwargs):
    levels.append(level)


def snapshot():
    rows = []
    with connection.cursor() as cursor:
        for statement in SNAPSHOT:
            cursor.execute(statement)
            rows += [r[0] for r in cursor.fetchall()]
    return rows


def team_pk(name):
    team = Team.objects.filter(name=name).first()
    return str(team.pk) if team else str(uuid.uuid4())


def project_pk(slug):
    project = Project.objects.filter(slug=slug).first()
    return project.pk if project else 999999


def user_pk(email):
    return User.objects.get(email=email).pk


def token_for(email):
    v = EmailVerification.objects.filter(user__email=email).order_by("-id").first()
    return v.token if v else "missing-token"


def nullbool(v):
    return {True: "true", False: "false", None: "unknown"}[v]


def run(step):
    op, s = step["op"], step
    anonymous = ("accept_new_user", "reset_password", "signup", "confirm_email", "resend_confirmation",
                 "request_reset_password")
    me = User.objects.get(email=s["as"]) if op not in anonymous else AnonymousUser()
    kind = "team" if op.startswith("team") else "project"
    group = {"team": tv, "project": pv}[kind]
    pk = (lambda: {"team_pk": team_pk(s["team"])} if kind == "team" else {"project_pk": project_pk(s["project"])})

    def post(data):
        r = rf.post("/", data)
        r.user, r.session = me, SessionStore()
        return r

    def get():
        r = rf.get("/")
        r.user, r.session = me, SessionStore()
        return r

    project_fields = lambda: {  # noqa
        k: s[k] for k in ("name", "visibility", "retention_max_event_count", "grouping_mechanism")}
    calls = {
        "team_create": lambda: (tv.team_new, post({"name": s["name"], "visibility": s["visibility"]}), {}),
        "team_edit": lambda: (tv.team_edit, post({"name": s["name"], "visibility": s["visibility"]}), pk()),
        "team_delete": lambda: (tv.team_edit, post({"action": "delete"}), pk()),
        "team_join": lambda: (tv.team_list, post({"action": f"join:{team_pk(s['team'])}"}),
                              {"ownership_filter": "mine"}),
        "team_leave": lambda: (tv.team_list, post({"action": f"leave:{team_pk(s['team'])}"}),
                               {"ownership_filter": "mine"}),
        "team_list": lambda: (tv.team_list, get(), {"ownership_filter": s["tab"]}),
        "project_create": lambda: (pv.project_new, post({"team": team_pk(s["team"]), **project_fields()}), {}),
        "project_edit": lambda: (pv.project_edit, post(project_fields()), pk()),
        "project_delete": lambda: (pv.project_edit, post({"action": "delete"}), pk()),
        "project_join": lambda: (pv.project_list, post({"action": f"join:{project_pk(s['project'])}"}),
                                 {"ownership_filter": "mine"}),
        "project_leave": lambda: (pv.project_list, post({"action": f"leave:{project_pk(s['project'])}"}),
                                  {"ownership_filter": "mine"}),
        "project_list": lambda: (pv.project_list, get(), {"ownership_filter": s["tab"]}),
        "sdk_setup": lambda: (pv.project_sdk_setup, get(), pk()),
        "accept_new_user": lambda: (
            tv.team_members_accept_new_user if s["group"] == "team" else pv.project_members_accept_new_user, get(),
            {**({"team_pk": team_pk(s["team"])} if s["group"] == "team" else {"project_pk": project_pk(s["project"])}),
             "token": token_for(s["token_for"])}),
        "reset_password": lambda: (uv.reset_password, post({"new_password1": s["password"],
                                                            "new_password2": s["password"]}),
                                   {"token": token_for(s["token_for"])}),
    }
    service = lambda: MessagingServiceConfig.objects.filter(  # noqa
        project_id=project_pk(s["project"]), display_name=s["service"]).first()
    service_id = lambda: service().pk if service() else 999999  # noqa
    config = lambda: {k: v.replace("@CAPTURE@", CAPTURE) for k, v in s.get("config", {}).items()}  # noqa
    calls.update({
        "service_add": lambda: (pv.project_messaging_service_add,
                                post({"display_name": s["display_name"], "kind": s["kind"], **config()}), pk()),
        "service_edit": lambda: (pv.project_messaging_service_edit,
                                 post({"display_name": s["display_name"], **config()}),
                                 {**pk(), "service_pk": service_id()}),
        "service_remove": lambda: (pv.project_alerts_setup, post({"action": f"remove:{service_id()}"}), pk()),
        "service_test": lambda: (pv.project_alerts_setup, post({"action": f"test:{service_id()}"}), pk()),
        "services": lambda: (pv.project_alerts_setup, get(), pk()),
    })
    calls.update({
        "signup": lambda: (uv.signup, post({"username": s["email"], "password1": s["password"],
                                            "password2": s["password"]}), {}),
        "confirm_email": lambda: (uv.confirm_email, post({}), {"token": token_for(s["token_for"])}),
        "resend_confirmation": lambda: (uv.resend_confirmation, post({"email": s["email"]}), {}),
        "request_reset_password": lambda: (uv.request_reset_password, post({"email": s["email"]}), {}),
        "preferences": lambda: (uv.preferences, post({
            "send_email_alerts": str(s["send_email_alerts"]), "theme_preference": s["theme_preference"],
            "language": s["language"]}), {}),
        "change_password": lambda: (uv.change_password, post({
            "old_password": s["old_password"], "new_password1": s["new_password"],
            "new_password2": s["new_password"]}), {}),
    })
    for k in ("team", "project"):
        mod = {"team": tv, "project": pv}[k]
        calls.update({
            f"{k}_invite": lambda mod=mod: (getattr(mod, f"{kind}_members_invite"),
                                            post({"email": s["email"], "role": s["role"]}), pk()),
            f"{k}_members": lambda mod=mod: (getattr(mod, f"{kind}_members"), get(), pk()),
            f"{k}_remove": lambda mod=mod: (getattr(mod, f"{kind}_members"),
                                            post({"action": f"remove:{user_pk(s['user'])}"}), pk()),
            f"{k}_reinvite": lambda mod=mod: (getattr(mod, f"{kind}_members"),
                                              post({"action": f"reinvite:{user_pk(s['user'])}"}), pk()),
            f"{k}_invite_link": lambda mod=mod: (getattr(mod, f"{kind}_members"),
                                                 post({"action": f"copy_invite_link:{user_pk(s['user'])}"}), pk()),
            f"{k}_member_settings": lambda mod=mod: (
                getattr(mod, f"{kind}_member_settings"),
                post({"role": s["role"], **({"send_email_alerts": nullbool(s["send_email_alerts"])}
                                            if "send_email_alerts" in s else {})}),
                {**pk(), "user_pk": str(user_pk(s["user"]))}),
            f"{k}_accept": lambda mod=mod: (getattr(mod, f"{kind}_members_accept"), post({"action": "accept"}), pk()),
            f"{k}_decline": lambda mod=mod: (getattr(mod, f"{kind}_members_accept"), post({"action": "decline"}), pk()),
        })

    result = {"n": s["n"], "outcome": None, "errors": None, "link": None, "read": None, "needs_password": None}
    translation.activate("en")  # a step changing preferences activates that user's language; bugsink-rs has English
    captured.clear()
    levels.clear()
    try:
        view, request, kwargs = calls[op]()
        response = view(request, **kwargs)
    except PermissionDenied:
        result["outcome"] = "forbidden"
    except (Http404, ObjectDoesNotExist):
        result["outcome"] = "not_found"
    except IntegrityError:
        result["outcome"] = "refused"
    else:
        form = captured.get("form")
        # the messaging service pages have a second, kind-specific form
        config_form = captured.get("config_form") or (captured.get("config_forms") or {}).get(
            captured.get("selected_config_form_kind"))
        form_errors = {**(form.errors if form is not None else {}),
                       **(config_form.errors if config_form is not None else {})}
        if any(level >= 30 for level in levels):
            result["outcome"] = "refused"
        elif op.endswith("member_settings") and response.get("Location", "").endswith("/members/accept/"):
            result["outcome"] = "refused"  # not accepted yet: sent to the invitation instead
        elif response.status_code == 200 and form_errors:
            result["outcome"] = "invalid"
            rename = {"signup": {"username": "email", "password1": "password", "password2": "password"},
                      "reset_password": {"new_password1": "password", "new_password2": "password"},
                      "change_password": {"new_password1": "new_password", "new_password2": "new_password"}}
            result["errors"] = {}
            for k, v in form_errors.items():
                messages = [re.sub("<[^>]*>", "", m) for m in v]  # upstream's are HTML; ours plain text
                result["errors"].setdefault(rename.get(op, {}).get(k, k), []).extend(messages)
        else:
            result["outcome"] = "ok"
        notice = captured.get("invite_link_notice")
        if notice is not None:
            result["link"] = notice.link
        if op == "accept_new_user" and response.status_code == 302:
            result["needs_password"] = "reset-password" in response["Location"]
        result["read"] = read(op, captured)
    result["snapshot"] = snapshot()
    return result


def read(op, c):
    if op == "team_list":
        return sorted([t.name, t.project_count, t.member_count,
                       [t.member.role, t.member.accepted, t.member.send_email_alerts] if getattr(t, "member", None)
                       else None] for t in c["team_list"])
    if op == "project_list":
        return sorted([p.name, p.team.name if p.team else None, p.member_count, p.open_issue_count,
                       [p.member.role, p.member.accepted, p.member.send_email_alerts] if getattr(p, "member", None)
                       else None] for p in c["project_list"])
    if op in ("team_members", "project_members"):
        return sorted([m.user.email, m.role, m.accepted, m.send_email_alerts, m.user.is_active] for m in c["members"])
    if op == "sdk_setup":
        return c["dsn"]
    if op == "services":
        return [[sc.display_name, sc.kind, json.loads(sc.config)] for sc in c["service_configs"].order_by("id")]
    return None


with mock.patch.object(tv, "render", capture), mock.patch.object(pv, "render", capture), \
        mock.patch.object(uv, "render", capture), \
        mock.patch.object(django.contrib.messages.api, "add_message", record_message):
    for line in sys.stdin:
        step = json.loads(line)
        if MODE in step.get("modes", [MODE]):
            print("STEP " + json.dumps(run(step), default=str), flush=True)
