# Runs inside upstream's `bugsink-manage shell` for accounts.sh; STEP selects what to do.
#   setup:  check the superuser bugsink-rs created (row and password), create users and projects in every access
#           situation, and print what upstream allows each user (a line starting with EXPECT, JSON).
#   verify: check the password bugsink-rs rehashed at login still works upstream, then change it (which must end the
#           bugsink-rs session).
import json
import os

from django.contrib.auth import authenticate
from django.contrib.auth.hashers import PBKDF2PasswordHasher

from projects.models import (
    Project, ProjectMembership, ProjectVisibility, projects_visible_to_user, user_has_project_membership_access)
from teams.models import Team, TeamMembership, TeamVisibility
from users.models import User

step = os.environ["STEP"]
USERS = {
    "admin@Example.COM": "admin-pw",
    "member@example.org": "member-pw",
    "outsider@example.org": "outsider-pw",
}

if step == "setup":
    # bugsink-rs's create-superuser against upstream's CREATE_SUPERUSER.
    User.objects.create_superuser(username="upstream@Example.COM", email="upstream@Example.COM", password="x")
    fields = ["is_superuser", "is_staff", "is_active", "first_name", "last_name", "send_email_alerts",
              "theme_preference", "language", "last_login"]
    ours, ref = User.objects.get(username="admin@Example.COM"), User.objects.get(username="upstream@Example.COM")
    assert [getattr(ours, f) for f in fields] == [getattr(ref, f) for f in fields], "superuser rows differ"
    assert ours.email == "admin@example.com", ours.email
    assert authenticate(username="admin@Example.COM", password="admin-pw") == ours, "our superuser can't log in"

    member = User.objects.create_user(username="member@example.org", email="member@example.org")
    member.password = PBKDF2PasswordHasher().encode("member-pw", "oldsalt", iterations=1000)  # outdated: rehashed
    member.save()
    User.objects.create_user(username="outsider@example.org", email="outsider@example.org", password="outsider-pw")
    User.objects.create_user(
        username="inactive@example.org", email="inactive@example.org", password="inactive-pw", is_active=False)

    team = Team.objects.create(name="T", visibility=TeamVisibility.JOINABLE)
    TeamMembership.objects.create(team=team, user=member, accepted=True)
    hidden = ProjectVisibility.TEAM_MEMBERS
    projects = {
        "Mine": Project.objects.create(name="Mine", visibility=hidden),
        "Invited": Project.objects.create(name="Invited", visibility=hidden),
        "Team": Project.objects.create(name="Team", team=team, visibility=hidden),
        "Joinable": Project.objects.create(name="Joinable", visibility=ProjectVisibility.JOINABLE),
        "Discoverable": Project.objects.create(name="Discoverable", visibility=ProjectVisibility.DISCOVERABLE),
        "Hidden": Project.objects.create(name="Hidden", visibility=hidden),
        "Deleted": Project.objects.create(name="Deleted", visibility=hidden, is_deleted=True),
    }
    ProjectMembership.objects.create(project=projects["Mine"], user=member, accepted=True)
    ProjectMembership.objects.create(project=projects["Invited"], user=member, accepted=False)
    ProjectMembership.objects.create(project=projects["Deleted"], user=member, accepted=True)

    expect = {}
    for username, password in USERS.items():
        user = User.objects.get(username=username)
        expect[username] = {
            "password": password,
            "visible": sorted(p.name for p in projects_visible_to_user(Project.objects.all(), user)),
            "access": {p.name: (p.id, user_has_project_membership_access(user, p)) for p in projects.values()},
        }
    print("EXPECT " + json.dumps(expect))

elif step == "verify":
    member = User.objects.get(username="member@example.org")
    assert member.password.startswith("pbkdf2_sha256$1000000$"), "not rehashed at login: " + member.password
    assert authenticate(username="member@example.org", password="member-pw") == member, "rehashed password fails"
    member.set_password("changed-pw")
    member.save()
    print("upstream verified the rehashed password and changed it")
