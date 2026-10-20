"""Robot de réessai du serveur Oracle (oracle/a1_robot.py), avec des doublures du kit d'Oracle."""

import base64
import os
import sys
import unittest
from types import SimpleNamespace

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "oracle"))

import a1_robot  # noqa: E402

KEY = "-----BEGIN PRIVATE KEY-----\nMIIabc\n-----END PRIVATE KEY-----"
PREVIEW = """[DEFAULT]
user=ocid1.user.oc1..aaaa
fingerprint=12:34:56:78:9a:bc:de:f0:12:34:56:78:9a:bc:de:f0
tenancy=ocid1.tenancy.oc1..bbbb
region=eu-marseille-1
key_file=<path to your private keyfile> # TODO
"""


class Models:
    """Doublure de oci.core.models : garde ce qu'on lui donne."""

    def __getattr__(self, name):
        return lambda **values: SimpleNamespace(kind=name, **values)


class ServiceError(Exception):
    def __init__(self, status, code, message):
        super().__init__(message)
        self.status, self.code, self.message = status, code, message


class Compute:
    def __init__(self, instances=(), refuse=None):
        self.instances = list(instances)
        self.refuse = refuse or (lambda details: None)
        self.launched = []

    def list_instances(self, compartment_id):
        return SimpleNamespace(data=self.instances)

    def list_images(self, **query):
        assert query["shape"] == "VM.Standard.A1.Flex" and query["operating_system_version"] == "24.04"
        return SimpleNamespace(data=[SimpleNamespace(id="image-ubuntu-arm")])

    def launch_instance(self, details):
        error = self.refuse(details)
        if error:
            raise error
        self.launched.append(details)
        return SimpleNamespace(data=SimpleNamespace(id="instance"))


class Network:
    def __init__(self, subnets=None):
        self.subnets = subnets if subnets is not None else [
            SimpleNamespace(id="subnet-private", lifecycle_state="AVAILABLE", prohibit_public_ip_on_vnic=True),
            SimpleNamespace(id="subnet-public", lifecycle_state="AVAILABLE", prohibit_public_ip_on_vnic=False),
        ]

    def list_subnets(self, compartment_id):
        return SimpleNamespace(data=self.subnets)


class Identity:
    def list_availability_domains(self, compartment_id):
        return SimpleNamespace(data=[SimpleNamespace(name="xyz:EU-MARSEILLE-1-AD-1")])


def robot(compute, network=None, tokens=None):
    tokens = tokens if tokens is not None else []
    config = a1_robot.config_from(PREVIEW, KEY)
    said = []

    def token():
        tokens.append("AJETON")
        return "AJETON"

    return a1_robot.Robot(config, compute, network or Network(), Identity(), token, Models(), said.append), said


CAPACITY = ServiceError(500, "InternalError", "Out of host capacity.")


class RobotTest(unittest.TestCase):
    def test_the_console_preview_becomes_a_configuration(self):
        config = a1_robot.config_from(PREVIEW, KEY)
        self.assertEqual((config["region"], config["tenancy"]), ("eu-marseille-1", "ocid1.tenancy.oc1..bbbb"))
        self.assertTrue(config["key_content"].startswith("-----BEGIN PRIVATE KEY-----"))
        with self.assertRaises(a1_robot.RobotError):
            a1_robot.config_from("user=ocid1.user\nregion=eu-paris-1", KEY)
        with self.assertRaises(a1_robot.RobotError):
            a1_robot.config_from(PREVIEW, "pas une clé")

    def test_an_existing_instance_means_nothing_to_do(self):
        compute = Compute(instances=[SimpleNamespace(shape="VM.Standard.A1.Flex", lifecycle_state="RUNNING")])
        tokens = []
        bot, _ = robot(compute, tokens=tokens)
        self.assertEqual(bot.run("o/claw-sessions"), "done")
        self.assertEqual((compute.launched, tokens), ([], []))
        # Une Micro (edt-bot) ou une A1 supprimée n'empêche rien.
        compute = Compute(instances=[SimpleNamespace(shape="VM.Standard.E2.1.Micro", lifecycle_state="RUNNING"),
                                     SimpleNamespace(shape="VM.Standard.A1.Flex", lifecycle_state="TERMINATED")])
        bot, _ = robot(compute)
        self.assertEqual(bot.run("o/claw-sessions"), "created")

    def test_full_hosts_lead_to_the_smaller_size_then_the_instance_is_launched(self):
        compute = Compute(refuse=lambda details: CAPACITY if details.shape_config.ocpus == 2.0 else None)
        bot, said = robot(compute)
        self.assertEqual(bot.run("o/claw-sessions"), "created")
        details = compute.launched[0]
        self.assertEqual((details.shape_config.ocpus, details.shape_config.memory_in_gbs), (1.0, 6.0))
        self.assertEqual((details.display_name, details.source_details.image_id), ("claw", "image-ubuntu-arm"))
        self.assertTrue(details.create_vnic_details.assign_public_ip)
        self.assertEqual(details.create_vnic_details.subnet_id, "subnet-public")
        script = base64.b64decode(details.metadata["user_data"]).decode("utf-8")
        self.assertIn("claw-cloud install --repo o/claw-sessions --token AJETON", script)
        self.assertTrue(script.startswith("#!/bin/bash"))
        self.assertIn("2 OCPU et 12 Go : pas de place pour l'instant.", said)
        # Rien d'identifiant dans ce que le robot dit (journal public).
        self.assertFalse(any("ocid1" in line or "AJETON" in line for line in said))

    def test_no_room_anywhere_means_waiting_with_one_token_asked(self):
        tokens = []
        bot, _ = robot(Compute(refuse=lambda details: CAPACITY), tokens=tokens)
        self.assertEqual(bot.run("o/claw-sessions"), "waiting")
        self.assertEqual(len(tokens), 1)

    def test_rate_limits_and_refusals(self):
        bot, _ = robot(Compute(refuse=lambda details: ServiceError(429, "TooManyRequests", "slow down")))
        self.assertEqual(bot.run("o/claw-sessions"), "busy")
        bot, _ = robot(Compute(refuse=lambda details: ServiceError(400, "LimitExceeded", "limit reached")))
        with self.assertRaises(a1_robot.RobotError) as raised:
            bot.run("o/claw-sessions")
        self.assertIn("LimitExceeded", str(raised.exception))

    def test_the_robot_switches_itself_off_when_done_or_stuck(self):
        from unittest import mock
        import json

        for outcome, switch in (("created", "done"), ("done", "done"), ("error", "off"), ("waiting", None)):
            calls = []
            with mock.patch.object(a1_robot, "github", lambda method, path, token, body=None: calls.append((method, path, body))):
                a1_robot.save_state("o/claw_code", "t", outcome, "détail")
            names = {body["name"]: body["value"] for _, _, body in calls if body}
            self.assertEqual(json.loads(names["ORACLE_ROBOT_STATE"])["outcome"], outcome)
            self.assertEqual(names.get("ORACLE_ROBOT"), switch, outcome)
            disabled = ("PUT", "/repos/o/claw_code/actions/workflows/oracle-a1.yml/disable", None) in calls
            self.assertEqual(disabled, switch is not None, outcome)

    def test_without_a_public_subnet_the_user_is_told_what_to_create(self):
        bot, _ = robot(Compute(), Network(subnets=[]))
        with self.assertRaises(a1_robot.RobotError) as raised:
            bot.run("o/claw-sessions")
        self.assertIn("assistant VCN", str(raised.exception))


if __name__ == "__main__":
    unittest.main()
