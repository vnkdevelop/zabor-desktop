import pathlib
import runpy
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = pathlib.Path(__file__).with_name("verify-profile-helper.py")
APPROVED = pathlib.Path(r"C:\Users\porad\AppData\Local\Temp\opencode")
RUNTIME = r"C:\Synthetic Electron Runtime\electron.exe"
METACHARACTERS = tuple('&|<>^%"!()') + ("\r", "\n", "\x00", "\t", "\x7f", "\u0085", "\u2028", "\u2029")


class CommandAdmissionTests(unittest.TestCase):
    def setUp(self):
        environment = mock.patch.dict("os.environ", {
            "SystemRoot": r"C:\Windows",
            "USERPROFILE": r"C:\Synthetic Tools",
            "TEMP": str(APPROVED),
            "TMP": str(APPROVED),
        }, clear=True)
        environment.start()
        self.addCleanup(environment.stop)

    def run_harness(self, runtime=RUNTIME, check_only=False, home=None):
        arguments = [str(SCRIPT), "--runtime", runtime]
        if check_only:
            arguments.append("--check-only")
        directories = []
        original_temporary_directory = tempfile.TemporaryDirectory

        def temporary_directory(*args, **kwargs):
            self.assertEqual(pathlib.Path(kwargs["dir"]), APPROVED)
            directory = original_temporary_directory(*args, **kwargs)
            directories.append(pathlib.Path(directory.name))
            return directory

        with mock.patch.object(sys, "argv", arguments), mock.patch(
            "subprocess.run"
        ) as launch, mock.patch(
            "tempfile.TemporaryDirectory", side_effect=temporary_directory
        ), mock.patch.object(pathlib.Path, "home", return_value=home or pathlib.Path(r"C:\Synthetic Tools")):
            namespace = runpy.run_path(str(SCRIPT), run_name="__main__")
        self.assertTrue(directories)
        self.assertTrue(all(not directory.exists() for directory in directories))
        return namespace, launch

    def test_runtime_metacharacters_are_rejected_before_any_launch_or_fixture(self):
        for character in METACHARACTERS:
            for check_only in (False, True):
                with self.subTest(character=repr(character), check_only=check_only):
                    arguments = [str(SCRIPT), "--runtime", "C:\\R" + character + "D\\electron.exe"]
                    if check_only:
                        arguments.append("--check-only")
                    with mock.patch.object(sys, "argv", arguments), mock.patch(
                        "subprocess.run"
                    ) as launch, mock.patch("tempfile.TemporaryDirectory") as fixture:
                        with self.assertRaisesRegex(ValueError, "^unsafe command argument$"):
                            runpy.run_path(str(SCRIPT), run_name="__main__")
                        launch.assert_not_called()
                        fixture.assert_not_called()

    def test_chained_runtime_fragment_is_never_submitted(self):
        value = r"C:\synthetic\electron.exe&echo injected"
        with mock.patch.object(sys, "argv", [str(SCRIPT), "--runtime", value]), mock.patch(
            "subprocess.run"
        ) as launch, mock.patch("tempfile.TemporaryDirectory") as fixture:
            with self.assertRaisesRegex(ValueError, "^unsafe command argument$"):
                runpy.run_path(str(SCRIPT), run_name="__main__")
            launch.assert_not_called()
            fixture.assert_not_called()

    def test_space_paths_and_every_payload_part_are_quoted(self):
        namespace, launch = self.run_harness()
        cargo = str(pathlib.Path(r"C:\Synthetic Tools") / ".cargo/bin/cargo.exe")
        prefix = 'cmd.exe /d /v:off /s /c "'
        expected = [
            prefix + '"' + cargo + '" "generate-lockfile" "--offline""',
            prefix + '"' + cargo + '" "check" "-j2" "--offline" "--locked""',
            prefix + '"' + cargo + '" "run" "-j2" "--offline" "--locked" "--" "'
            + RUNTIME + '" "' + str(namespace["root"] / "scripts/migration") + '" "' + str(APPROVED) + '""',
        ]
        self.assertEqual([call.args[0] for call in launch.call_args_list], expected)
        for call in launch.call_args_list:
            self.assertIs(call.kwargs["shell"], False)
            self.assertIs(call.kwargs["check"], True)
            self.assertTrue(str(call.kwargs["cwd"]).startswith(str(APPROVED)))

    def test_check_only_does_not_schedule_a_runtime(self):
        _, launch = self.run_harness(check_only=True)
        self.assertEqual(launch.call_count, 2)
        self.assertTrue(all(RUNTIME not in call.args[0] for call in launch.call_args_list))

    def test_shared_boundary_rejects_unsafe_executable_arguments_and_cwd(self):
        namespace, _ = self.run_harness(check_only=True)
        run_cmd = namespace["run_cmd"]
        for character in METACHARACTERS:
            bad = "synthetic" + character + "fragment"
            for arguments, directory in (
                ([bad, "check"], r"C:\Synthetic Folder"),
                ([r"C:\Synthetic Tools\cargo.exe", bad], r"C:\Synthetic Folder"),
                ([r"C:\Synthetic Tools\cargo.exe", "check"], bad),
            ):
                with self.subTest(character=repr(character), arguments=arguments), mock.patch(
                    "subprocess.run"
                ) as launch:
                    with self.assertRaisesRegex(ValueError, "^unsafe command argument$"):
                        run_cmd(arguments, cwd=directory, env={})
                    launch.assert_not_called()

    def test_derived_cargo_path_is_rejected_before_any_launch(self):
        with mock.patch.object(sys, "argv", [str(SCRIPT), "--runtime", RUNTIME]), mock.patch.object(
            pathlib.Path, "home", return_value=pathlib.Path(r"C:\Synthetic%Tools")
        ), mock.patch("subprocess.run") as launch, mock.patch("tempfile.TemporaryDirectory") as fixture:
            with self.assertRaisesRegex(ValueError, "^unsafe command argument$"):
                runpy.run_path(str(SCRIPT), run_name="__main__")
            launch.assert_not_called()
            fixture.assert_not_called()

    def test_quoted_trailing_backslashes_are_preserved(self):
        namespace, _ = self.run_harness(check_only=True)
        value = "C:\\Synthetic Folder\\"
        with mock.patch("subprocess.run") as launch:
            namespace["run_cmd"]([r"C:\Synthetic Tools\cargo.exe", value], cwd=APPROVED, env={})
            self.assertEqual(
                launch.call_args.args[0],
                'cmd.exe /d /v:off /s /c ""C:\\Synthetic Tools\\cargo.exe" "C:\\Synthetic Folder\\\\""',
            )


if __name__ == "__main__":
    unittest.main()
