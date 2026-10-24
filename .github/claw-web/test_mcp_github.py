"""Essais du serveur MCP `github` (mcp_github.py), avec une fausse API GitHub.

    cd .github/claw-web && python -m unittest test_mcp_github
"""

import base64
import json
import tempfile
import unittest
import urllib.parse
from pathlib import Path
from unittest import mock

import mcp_github
import on_github
from common import GitHubError
from mcp_server import Context, Server

NAMES = """actions_get actions_list actions_run_trigger add_comment_to_pending_review add_issue_comment
add_reply_to_pull_request_comment create_branch create_or_update_file create_pull_request create_repository
delete_file disable_pr_auto_merge enable_pr_auto_merge fork_repository get_check_run get_commit get_file_contents
get_job_logs get_label get_latest_release get_me get_release_by_tag get_tag get_team_members get_teams issue_read
issue_write list_branches list_commits list_issue_fields list_issue_types list_issues list_pull_requests
list_releases list_repository_collaborators list_tags merge_pull_request pull_request_read
pull_request_review_write push_files request_copilot_review resolve_review_thread run_secret_scanning search_code
search_commits search_issues search_pull_requests search_repositories search_users sub_issue_write
unresolve_review_thread update_issue_comment update_pull_request update_pull_request_branch""".split()


class FakeGitHub:
    """Réponses par (méthode, chemin sans « /repos/o/r ») ; une liste est consommée dans l'ordre,
    une exception levée ; une route absente répond 404. Garde les appels."""

    def __init__(self, routes=None):
        self.routes = dict(routes or {})
        self.calls = []

    def __call__(self, method, path, token, body=None):
        self.calls.append((method, path, body))
        key = (method, path.split("/repos/o/r", 1)[-1])
        if key not in self.routes:
            raise GitHubError(f"{method} {path} : HTTP 404", 404, "Not Found")
        answer = self.routes[key]
        if isinstance(answer, list):
            answer = answer.pop(0)
        if isinstance(answer, GitHubError):
            raise answer
        return answer

    def paths(self, method=None):
        return [path for verb, path, _ in self.calls if method in (None, verb)]


def query_of(path):
    return urllib.parse.parse_qs(urllib.parse.urlsplit(path).query)


class Base(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.requests = Path(folder.name) / "github-requests.json"
        patch = mock.patch.object(on_github, "REQUESTS_FILE", self.requests)
        patch.start()
        self.addCleanup(patch.stop)
        self.raw = {}

    def server(self, routes=None, local=False):
        self.fake = FakeGitHub(routes)

        def fake_raw(path, token, accept="application/vnd.github.raw"):
            self.fake.calls.append(("RAW", path, accept))
            return self.raw[path.split("/repos/o/r", 1)[-1]]

        context = Context(call=self.fake, raw=fake_raw, repo="o/r", base="main", branch="claw/session-3",
                          repo_token="t", requests_file=self.requests, local=local)
        return Server("github", mcp_github.tools(context))

    def call(self, server, tool_name, /, **arguments):
        result = server.call(tool_name, arguments)
        return bool(result.get("isError")), result["content"][0]["text"]

    def ok(self, server, tool_name, /, **arguments):
        error, text = self.call(server, tool_name, **arguments)
        self.assertFalse(error, text)
        try:
            return json.loads(text)
        except ValueError:
            return text


class CatalogTest(Base):
    def test_names_order_and_schemas(self):
        listed = mcp_github.tools(Context())
        self.assertEqual([tool.name for tool in listed], NAMES)
        for tool in listed:
            properties = tool.schema["properties"]
            self.assertLessEqual(set(tool.schema["required"]), set(properties), tool.name)
            self.assertTrue(tool.title and tool.description, tool.name)
        read_only = {tool.name for tool in listed if tool.read_only}
        self.assertIn("run_secret_scanning", read_only)
        self.assertIn("get_check_run", read_only)
        self.assertNotIn("merge_pull_request", read_only)
        self.assertEqual(Server("github", listed).listing()[0]["name"], "actions_get")

    def test_notes_on_restricted_tools(self):
        notes = {tool.name: tool.note for tool in mcp_github.tools(Context())}
        self.assertEqual(notes["merge_pull_request"], "Pull request de la session seulement, fusionnée par Claw "
                                                      "après la poussée, quand vous le demandez")
        for name in ("push_files", "create_or_update_file", "delete_file", "search_code", "create_repository",
                     "run_secret_scanning", "update_pull_request_branch", "enable_pr_auto_merge"):
            self.assertTrue(notes[name], name)


class ScopeTest(Base):
    def test_another_repository_is_refused_and_get_me_is_not(self):
        server = self.server({("GET", "/user"): {"login": "me", "id": 1, "html_url": "https://github.com/me",
                                                 "avatar_url": "a", "name": "Moi"}})
        error, text = self.call(server, "issue_read", method="get", owner="x", repo="y", issue_number=1)
        self.assertTrue(error)
        self.assertIn("this session's repository, o/r", text)
        self.assertEqual(self.fake.calls, [])
        me = self.ok(server, "get_me")
        self.assertEqual((me["login"], me["details"]["name"]), ("me", "Moi"))
        # La casse ne compte pas.
        self.raw = {}
        server = self.server({("GET", "/labels/bug"): {"id": 3, "name": "bug", "color": "f00", "url": "u"}})
        self.assertEqual(self.ok(server, "get_label", owner="O", repo="R", name="bug")["name"], "bug")

    def test_search_queries_stay_on_the_repository(self):
        server = self.server()
        self.fake.routes[("GET", "/search/code?q=foo+repo%3Ao%2Fr&page=1&per_page=30")] = {
            "total_count": 1, "items": [{"name": "a.py", "path": "src/a.py", "sha": "s", "html_url": "h",
                                         "repository": {"full_name": "o/r"}}]}
        found = self.ok(server, "search_code", query="foo")
        self.assertEqual(found["items"][0]["repository"], "o/r")
        for query in ("bug repo:x/y", "bug org:other", "bug user:someone"):
            error, text = self.call(server, "search_issues", query=query)
            self.assertTrue(error, query)
            self.assertIn("out of reach", text)
        self.fake.routes.clear()
        self.call(server, "search_issues", query="crash repo:O/R")
        self.call(server, "search_pull_requests", query="fix org:o")
        issues, pulls = self.fake.paths("GET")[-2:]
        self.assertEqual(query_of(issues)["q"], ["is:issue crash repo:O/R"])
        self.assertEqual(query_of(pulls)["q"], ["is:pr fix repo:o/r"])

    def test_creating_or_forking_a_repository_is_for_the_user(self):
        server = self.server()
        error, text = self.call(server, "create_repository", name="new")
        self.assertTrue(error)
        self.assertIn("Créer un dépôt", text)
        error, text = self.call(server, "fork_repository", owner="o", repo="r")
        self.assertTrue(error)
        self.assertEqual(self.fake.calls, [])
        local = self.server({("POST", "/user/repos"): {"full_name": "me/new", "private": True}}, local=True)
        self.assertEqual(self.ok(local, "create_repository", name="new")["full_name"], "me/new")
        self.assertEqual(self.fake.calls[0][2], {"name": "new", "private": True, "auto_init": False})


class FilesTest(Base):
    ROUTES = {("GET", ""): {"default_branch": "trunk"}}

    def test_writes_refused_on_the_session_branch_and_the_base(self):
        server = self.server(dict(self.ROUTES))
        for branch, said in (("claw/session-3", "edit your working copy"), ("main", "nothing lands on the base"),
                             ("trunk", "nothing lands on the base"), ("refs/heads/main", "base")):
            error, text = self.call(server, "create_or_update_file", owner="o", repo="r", path="a.txt",
                                    content="x", message="m", branch=branch)
            self.assertTrue(error, branch)
            self.assertIn(said, text)
        error, text = self.call(server, "delete_file", owner="o", repo="r", path="a.txt", message="m",
                                branch="claw/session-3")
        self.assertTrue(error)
        self.assertFalse(any(verb in ("PUT", "POST", "PATCH") for verb, _, _ in self.fake.calls))

    def test_write_on_another_branch(self):
        server = self.server({**self.ROUTES, ("PUT", "/contents/docs/a.txt"): {
            "content": {"name": "a.txt", "path": "docs/a.txt", "sha": "b2", "url": "x"},
            "commit": {"sha": "c9", "message": "m", "html_url": "h"}}})
        found = self.ok(server, "create_or_update_file", owner="o", repo="r", path="docs/a.txt", content="héllo",
                        message="m", branch="feature")
        self.assertEqual(found["commit"]["sha"], "c9")
        put = [call for call in self.fake.calls if call[0] == "PUT"][0]
        self.assertEqual(put[2], {"message": "m", "branch": "feature",
                                  "content": base64.b64encode("héllo".encode()).decode()})

    def test_existing_file_needs_its_sha(self):
        server = self.server({**self.ROUTES, ("GET", "/contents/a.txt?ref=feature"): {"type": "file", "sha": "old"}})
        error, text = self.call(server, "create_or_update_file", owner="o", repo="r", path="a.txt", content="x",
                                message="m", branch="feature")
        self.assertTrue(error)
        self.assertIn("provide the current file's SHA", text)

    def test_push_files_with_the_git_data_api(self):
        server = self.server({**self.ROUTES,
                              ("GET", "/git/ref/heads/feature/x"): {"ref": "refs/heads/feature/x", "object": {"sha": "c1"}},
                              ("GET", "/git/commits/c1"): {"sha": "c1", "tree": {"sha": "t1"}},
                              ("POST", "/git/trees"): {"sha": "t2"},
                              ("POST", "/git/commits"): {"sha": "c2", "html_url": "h2"},
                              ("PATCH", "/git/refs/heads/feature/x"): {"ref": "refs/heads/feature/x"}})
        found = self.ok(server, "push_files", owner="o", repo="r", branch="feature/x", message="deux fichiers",
                        files=[{"path": "a.txt", "content": "A"}, {"path": "b/c.txt", "content": "C"}])
        self.assertEqual((found["sha"], found["files"]), ("c2", 2))
        bodies = {(verb, path.split("/repos/o/r")[-1]): body for verb, path, body in self.fake.calls}
        self.assertEqual(bodies[("POST", "/git/trees")]["base_tree"], "t1")
        self.assertEqual([entry["path"] for entry in bodies[("POST", "/git/trees")]["tree"]], ["a.txt", "b/c.txt"])
        self.assertEqual(bodies[("POST", "/git/commits")], {"message": "deux fichiers", "tree": "t2", "parents": ["c1"]})
        self.assertEqual(bodies[("PATCH", "/git/refs/heads/feature/x")], {"sha": "c2", "force": False})

    def test_get_file_contents_file_and_directory(self):
        server = self.server({
            ("GET", "/contents/src/a.py?ref=claw%2Fsession-3"): {
                "type": "file", "path": "src/a.py", "sha": "abc", "size": 11, "encoding": "base64",
                "content": base64.b64encode(b"hello world").decode()},
            ("GET", "/contents/src"): [[{"type": "file", "name": "a.py", "path": "src/a.py", "size": 11, "sha": "abc",
                                         "url": "u", "git_url": "g", "html_url": "h", "download_url": "d"}]] * 2})
        text = self.ok(server, "get_file_contents", owner="o", repo="r", path="src/a.py", ref="claw/session-3")
        self.assertIn("SHA: abc", text)
        self.assertTrue(text.endswith("hello world"))
        self.assertIn("last pushed state", text)
        listing = self.ok(server, "get_file_contents", owner="o", repo="r", path="/src/")
        self.assertEqual(listing[0]["name"], "a.py")
        self.assertNotIn("git_url", listing[0])
        self.assertEqual(self.ok(server, "get_file_contents", owner="o", repo="r", path="src", fields=["name"]),
                         [{"name": "a.py"}])


class MergeTest(Base):
    def pull(self, number, head):
        return {"number": number, "state": "open", "merged_at": None,
                "head": {"ref": head, "repo": {"full_name": "o/r"}}, "base": {"ref": "main"}}

    def test_session_pull_request_is_merged_by_claw_after_the_push(self):
        server = self.server({("GET", "/pulls/7"): self.pull(7, "claw/session-3"),
                              ("GET", "/pulls/8"): self.pull(8, "feature")})
        text = self.ok(server, "merge_pull_request", owner="o", repo="r", pullNumber=7)
        self.assertIn("Claw commits and pushes your work, then merges pull request #7", text)
        self.assertIn("without claiming it is done", text)
        self.assertEqual(on_github.load_requests(), {"merge": True})
        self.assertNotIn("PUT", [verb for verb, _, _ in self.fake.calls])
        self.requests.unlink()
        error, text = self.call(server, "merge_pull_request", owner="o", repo="r", pullNumber=8)
        self.assertTrue(error)
        self.assertIn("done by the user", text)
        self.assertEqual(on_github.load_requests(), {})

    def test_local_mode_merges(self):
        server = self.server({("PUT", "/pulls/8/merge"): {"merged": True, "sha": "m1", "message": "Merged"}}, local=True)
        found = self.ok(server, "merge_pull_request", owner="o", repo="r", pullNumber=8, merge_method="squash",
                        commit_title="T")
        self.assertTrue(found["merged"])
        self.assertEqual(self.fake.calls, [("PUT", "/repos/o/r/pulls/8/merge", {"commit_title": "T",
                                                                                "merge_method": "squash"})])
        self.assertEqual(on_github.load_requests(), {})

    def test_update_branch_refused_for_the_session_pull_request(self):
        server = self.server({("GET", "/pulls/7"): self.pull(7, "claw/session-3"),
                              ("GET", "/pulls/8"): self.pull(8, "feature"),
                              ("PUT", "/pulls/8/update-branch"): {"message": "Updating pull request branch."}})
        error, text = self.call(server, "update_pull_request_branch", owner="o", repo="r", pullNumber=7)
        self.assertTrue(error)
        self.assertIn("git merge origin/main", text)
        self.assertEqual(self.ok(server, "update_pull_request_branch", owner="o", repo="r", pullNumber=8)["message"],
                         "Updating pull request branch.")


class ReadTest(Base):
    def test_issue_read_get_and_comments(self):
        server = self.server({
            ("GET", "/issues/5"): {"id": 50, "number": 5, "title": "Bug", "body": "b", "state": "open",
                                   "node_id": "I_5", "user": {"login": "ana", "avatar_url": "x"},
                                   "labels": [{"name": "bug", "url": "u"}], "url": "api"},
            ("POST", "/graphql"): {"data": {"node": {"parent": None,
                                                     "subIssuesSummary": {"total": 2, "completed": 1, "percentCompleted": 50},
                                                     "closedByPullRequestsReferences": {"totalCount": 0, "nodes": []}}}},
            ("GET", "/issues/5/comments?page=2&per_page=5"): [[{"id": 9, "body": "salut", "user": {"login": "bo"},
                                                               "html_url": "h", "node_id": "n"}]]})
        issue = self.ok(server, "issue_read", method="get", owner="o", repo="r", issue_number=5)
        self.assertEqual((issue["user"], issue["labels"], issue["has_children"]), ("ana", ["bug"], True))
        self.assertEqual(issue["sub_issues_summary"]["percent_completed"], 50)
        self.assertNotIn("node_id", issue)
        comments = self.ok(server, "issue_read", method="get_comments", owner="o", repo="r", issue_number=5.0,
                           page=2, perPage=5)
        self.assertEqual(comments, [{"id": 9, "html_url": "h", "body": "salut", "user": "bo"}])

    def test_check_run_window_and_envelope(self):
        text = "x" * 30 + "END"
        server = self.server({("GET", "/check-runs/77"): {
            "id": 77, "name": "lint", "status": "completed", "conclusion": "failure", "details_url": "https://ci",
            "app": {"name": "CI app"}, "output": {"title": "2 errors", "summary": "s", "text": text}}})
        run = self.ok(server, "get_check_run", owner="o", repo="r", checkRunId=77, textLimit=16)
        self.assertEqual(run["name"], "<untrusted_external_data>lint</untrusted_external_data>")
        self.assertEqual(run["output"]["text"], f"<untrusted_external_data>{'x' * 16}</untrusted_external_data>")
        self.assertEqual(run["text_window"], "[showing bytes 0-16 of 33 total] pass textOffset=16 to read on")
        self.assertEqual(run["app"], "CI app")
        last = self.ok(server, "get_check_run", owner="o", repo="r", checkRunId=77, textLimit=16, textOffset=30)
        self.assertEqual(last["text_window"], "[showing bytes 30-33 of 33 total]")
        self.assertIn("END", last["output"]["text"])

    def test_job_logs_tail(self):
        server = self.server()
        self.raw["/actions/jobs/4/logs"] = "\n".join(f"line {n}" for n in range(1, 11)).encode()
        found = self.ok(server, "get_job_logs", owner="o", repo="r", job_id=4, return_content=True, tail_lines=3)
        self.assertEqual(found["logs_content"], "line 8\nline 9\nline 10")
        self.assertEqual(found["original_length"], 10)

    def test_failed_job_logs_of_a_run(self):
        server = self.server({("GET", "/actions/runs/9/jobs?filter=latest&per_page=100"): {"jobs": [
            {"id": 1, "name": "build", "conclusion": "success"}, {"id": 2, "name": "test", "conclusion": "failure"}]}})
        self.raw["/actions/jobs/2/logs"] = b"boom"
        found = self.ok(server, "get_job_logs", owner="o", repo="r", run_id=9, failed_only=True, return_content=True)
        self.assertEqual((found["failed_jobs"], found["logs"][0]["job_name"]), (1, "test"))
        self.assertEqual(found["logs"][0]["logs_content"], "boom")

    def test_signed_urls_come_from_the_redirect(self):
        server = self.server()
        with mock.patch.object(mcp_github, "fetch_location", return_value="https://signed/log") as fetch:
            found = self.ok(server, "get_job_logs", owner="o", repo="r", job_id=4)
        self.assertEqual(found["logs_url"], "https://signed/log")
        fetch.assert_called_once_with("/repos/o/r/actions/jobs/4/logs", "t")


class SecretsTest(Base):
    def test_known_secrets_are_found_and_redacted(self):
        token = "ghp_" + "a1B2c3D4e5" * 3 + "F6g7H8"
        server = self.server()
        found = self.ok(server, "run_secret_scanning", owner="o", repo="r", files=[
            "print('ok')\n", f"# config\nGITHUB = '{token}'\naws_key = AKIAIOSFODNN7EXAMPLE\n"])
        kinds = {(item["file_index"], item["line"], item["secret_type"]) for item in found["findings"]}
        self.assertEqual(kinds, {(1, 2, "GitHub personal access token"), (1, 3, "AWS access key ID")})
        text = json.dumps(found)
        self.assertNotIn(token, text)
        self.assertNotIn("AKIAIOSFODNN7EXAMPLE", text)
        self.assertIn("ghp_********", text)

    def test_clean_text_has_no_findings(self):
        server = self.server()
        found = self.ok(server, "run_secret_scanning", owner="o", repo="r",
                        files="password = os.environ['PASSWORD']\ntoken: \"changeme-later\"\nurl = 'https://example.com/a'\n")
        self.assertEqual(found["findings"], [])
        self.assertIn("No secrets found", found["message"])

    def test_more_formats(self):
        server = self.server()
        found = self.ok(server, "run_secret_scanning", owner="o", repo="r", files=[
            "-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY-----",
            "DB=postgres://admin:S3cr3tPassw0rd@db:5432/x",
            'api_key = "Zq8x!Lm2Pw9R"'])
        self.assertEqual([item["secret_type"] for item in found["findings"]],
                         ["Private key", "Credentials in URL", "Generic secret assignment"])
        self.assertNotIn("S3cr3tPassw0rd", json.dumps(found))


class GraphQLTest(Base):
    def test_errors_in_a_200_answer_are_refusals(self):
        server = self.server({("POST", "/graphql"): {"data": None, "errors": [
            {"message": "Could not resolve to a node with the global id of 'PRRT_x'"}]}})
        error, text = self.call(server, "resolve_review_thread", threadID="PRRT_x")
        self.assertTrue(error)
        self.assertIn("GitHub GraphQL error: Could not resolve", text)

    def test_thread_of_the_repository_is_resolved_and_another_refused(self):
        thread = {"__typename": "PullRequestReviewThread", "repository": {"nameWithOwner": "o/r"}}
        server = self.server({("POST", "/graphql"): [
            {"data": {"node": thread}}, {"data": {"resolveReviewThread": {"thread": {"isResolved": True}}}},
            {"data": {"node": {**thread, "repository": {"nameWithOwner": "x/y"}}}}]})
        self.assertEqual(self.ok(server, "resolve_review_thread", threadID="PRRT_1"), "review thread resolved successfully")
        self.assertEqual(self.fake.calls[1][2]["variables"], {"input": {"threadId": "PRRT_1"}})
        error, text = self.call(server, "unresolve_review_thread", threadID="PRRT_2")
        self.assertTrue(error)
        self.assertIn("x/y", text)
        self.assertEqual(len(self.fake.calls), 3)


if __name__ == "__main__":
    unittest.main()
