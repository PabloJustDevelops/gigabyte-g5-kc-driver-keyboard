"""Unit tests for the fan logic that is worth being careful about.

Everything here is pure: no EC, no sysfs, no root. What it guards is the part
that decides *what duty to command* — the curve interpolation, the validation
that refuses a curve which dips, and the tacho-to-rpm conversion, which is easy
to get wrong by a factor of ~23 (the register holds a period, not a speed).

Run with:  python -m unittest discover -s tests -v
"""
import importlib.util
import json
import os
import shutil
import sys
import tempfile
import time
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
_spec = importlib.util.spec_from_file_location(
    "g5fan", os.path.join(ROOT, "src", "g5fan.py"))
g5fan = importlib.util.module_from_spec(_spec)
sys.modules["g5fan"] = g5fan
_spec.loader.exec_module(g5fan)


class TestUnitConversion(unittest.TestCase):
    def test_pct_to_byte_ends(self):
        self.assertEqual(g5fan.pct_to_byte(0), 0)
        self.assertEqual(g5fan.pct_to_byte(100), 255)

    def test_pct_to_byte_clamps(self):
        self.assertEqual(g5fan.pct_to_byte(-20), 0)
        self.assertEqual(g5fan.pct_to_byte(1000), 255)

    def test_pct_to_byte_is_monotonic(self):
        levels = [g5fan.pct_to_byte(p) for p in range(101)]
        self.assertEqual(levels, sorted(levels))

    def test_byte_to_pct(self):
        self.assertEqual(g5fan.byte_to_pct(255), 100)
        self.assertEqual(g5fan.byte_to_pct(0), 0)
        self.assertIsNone(g5fan.byte_to_pct(None))


class TestRpm(unittest.TestCase):
    """2156220 / period, with a stopped fan reporting 0 or an 0xFFxx sentinel.

    The reference values are the ones a live G5 KC actually reports for its
    CPU fan at a 35 % duty.
    """

    def test_live_cpu_fan(self):
        self.assertEqual(g5fan.rpm_from_tach(972), 2218)

    def test_stopped(self):
        self.assertEqual(g5fan.rpm_from_tach(0), 0)
        self.assertEqual(g5fan.rpm_from_tach(g5fan.TACH_STOPPED), 0)
        self.assertEqual(g5fan.rpm_from_tach(0xFFF0), 0)

    def test_unavailable(self):
        self.assertIsNone(g5fan.rpm_from_tach(None))

    def test_faster_fan_means_shorter_period(self):
        self.assertGreater(g5fan.rpm_from_tach(500), g5fan.rpm_from_tach(1500))


class TestCurveInterpolation(unittest.TestCase):
    CURVE = ((40, 10), (60, 30), (80, 70), (100, 100))

    def test_clamps_below_and_above(self):
        self.assertEqual(g5fan.duty_for(self.CURVE, 0), 10)
        self.assertEqual(g5fan.duty_for(self.CURVE, 40), 10)
        self.assertEqual(g5fan.duty_for(self.CURVE, 100), 100)
        self.assertEqual(g5fan.duty_for(self.CURVE, 150), 100)

    def test_midpoints_interpolate(self):
        # exactly on a point
        self.assertEqual(g5fan.duty_for(self.CURVE, 60), 30)
        # halfway between (60,30) and (80,70)
        self.assertEqual(g5fan.duty_for(self.CURVE, 70), 50)
        # halfway between (40,10) and (60,30)
        self.assertEqual(g5fan.duty_for(self.CURVE, 50), 20)

    def test_never_falls(self):
        duties = [g5fan.duty_for(self.CURVE, t) for t in range(0, 121)]
        self.assertEqual(duties, sorted(duties))

    def test_two_point_curve(self):
        self.assertEqual(g5fan.duty_for(((50, 0), (90, 100)), 70), 50)


class TestValidateCurve(unittest.TestCase):
    def test_accepts_a_sensible_curve(self):
        pts = ((45, 15), (60, 25), (92, 100))
        self.assertEqual(g5fan.validate_curve(pts), pts)

    def test_refuses_duty_that_falls(self):
        with self.assertRaises(ValueError) as cm:
            g5fan.validate_curve(((50, 60), (70, 20)))
        self.assertIn("fall", str(cm.exception))

    def test_refuses_temperature_that_does_not_rise(self):
        with self.assertRaises(ValueError) as cm:
            g5fan.validate_curve(((70, 20), (50, 40)))
        self.assertIn("rise", str(cm.exception))
        with self.assertRaises(ValueError):
            g5fan.validate_curve(((70, 20), (70, 40)))

    def test_refuses_a_single_point(self):
        with self.assertRaises(ValueError):
            g5fan.validate_curve(((50, 20),))

    def test_refuses_more_than_five_points(self):
        with self.assertRaises(ValueError) as cm:
            g5fan.validate_curve(tuple((40 + i, 10 * i) for i in range(6)))
        self.assertIn("2..5", str(cm.exception))

    def test_refuses_out_of_range(self):
        with self.assertRaises(ValueError):
            g5fan.validate_curve(((50, 20), (200, 40)))
        with self.assertRaises(ValueError):
            g5fan.validate_curve(((50, 20), (70, 140)))

    def test_flat_duty_is_allowed(self):
        # Full speed from the start is a legitimate curve.
        self.assertEqual(g5fan.validate_curve(((40, 100), (90, 100))),
                         ((40, 100), (90, 100)))


class TestParsePoint(unittest.TestCase):
    def test_pair(self):
        self.assertEqual(g5fan.parse_point("55:30"), (55, 30))

    def test_rejects_junk(self):
        for bad in ("55", "abc", "55:", ":30"):
            with self.assertRaises(ValueError):
                g5fan.parse_point(bad)


class TestParseMode(unittest.TestCase):
    def test_names(self):
        self.assertEqual(g5fan.parse_mode("auto"), ("auto", None))
        self.assertEqual(g5fan.parse_mode("MAX"), ("turbo", None))
        self.assertEqual(g5fan.parse_mode("MaxQ"), ("maxq", None))

    def test_manual_forms(self):
        self.assertEqual(g5fan.parse_mode("manual:60"), ("manual", 60))
        self.assertEqual(g5fan.parse_mode("manual 60"), ("manual", 60))

    def test_manual_bounds(self):
        with self.assertRaises(ValueError):
            g5fan.parse_mode("manual:200")

    def test_unknown(self):
        with self.assertRaises(ValueError):
            g5fan.parse_mode("ludicrous")


class TestStoredCurve(unittest.TestCase):
    def test_round_trip(self):
        pts = [[45, 15], [60, 25], [92, 100]]
        self.assertEqual(g5fan.as_curve(pts),
                         ((45, 15), (60, 25), (92, 100)))

    def test_rejects_an_older_shape(self):
        # The first cut of g5fan stored a flat [T2, D2, T3, D3]; it must be
        # dropped rather than driven onto the fans.
        self.assertIsNone(g5fan.as_curve([55, 25, 70, 50]))

    def test_rejects_nonsense(self):
        for bad in (None, [], ["x:y"], [[50, 20]], [[70, 20], [60, 40]]):
            self.assertIsNone(g5fan.as_curve(bad))

    def test_preset_is_always_valid(self):
        for name, points in g5fan.PROFILES.items():
            with self.subTest(preset=name):
                self.assertEqual(g5fan.validate_curve(points), points)


class RecordingBackend:
    """A FanBackend stand-in that records instead of touching hardware.

    It implements only what Supervisor.tick actually calls, which is the point:
    the daemon must not need to read anything back to do its job.
    """

    name = "test"
    root = None

    def __init__(self):
        self.duties: list[tuple[int, int]] = []
        self.autos = 0
        self.temps = {"cpu": 50.0, "gpu": 45.0}
        self.duty_bytes = {1: 89, 2: 89}     # 35 %
        self.tach_periods = {1: 972, 2: 1020}

    def cpu_temp(self):
        return self.temps["cpu"]

    def gpu_temp(self):
        return self.temps["gpu"]

    def duty(self, fan):
        return self.duty_bytes.get(fan)

    def tach(self, fan):
        return self.tach_periods.get(fan)

    def set_duties(self, cpu_pct, gpu_pct):
        self.duties.append((cpu_pct, gpu_pct))

    def auto(self):
        self.autos += 1


def _patch(test, **attrs):
    """Set module attributes for one test and put them back afterwards."""
    for name, value in attrs.items():
        real = getattr(g5fan, name)
        test.addCleanup(setattr, g5fan, name, real)
        setattr(g5fan, name, value)


class TestSupervisor(unittest.TestCase):
    """The daemon's two jobs: follow the curve, and never let the CPU cook."""

    def setUp(self):
        self.state = {"mode": "silent", "duty": None, "curve": None}
        self.dir = tempfile.mkdtemp(prefix="g5fan-test-")
        self.addCleanup(shutil.rmtree, self.dir, True)
        _patch(self,
               load_state=lambda: dict(self.state),
               SNAPSHOT_PATH=os.path.join(self.dir, "status.json"))

        self.b = RecordingBackend()
        self.sup = g5fan.Supervisor(interval=1, ceiling=95.0)

    def test_curve_is_driven_from_each_fans_own_temperature(self):
        self.sup.tick(self.b, 95.0)
        self.assertEqual(len(self.b.duties), 1)
        cpu, gpu = self.b.duties[0]
        self.assertEqual(cpu, g5fan.duty_for(g5fan.PROFILES["silent"], 50))
        self.assertEqual(gpu, g5fan.duty_for(g5fan.PROFILES["silent"], 45))

    def test_a_steady_temperature_does_not_rewrite(self):
        self.sup.tick(self.b, 95.0)
        self.sup.tick(self.b, 95.0)
        self.assertEqual(len(self.b.duties), 1)

    def test_returning_to_a_curve_after_auto_writes_again(self):
        # silent -> auto -> silent, at an unchanged temperature. The targets
        # are identical to the ones written before, but the EC was handed back
        # to its own curve in between, so they have to be re-sent.
        self.sup.tick(self.b, 95.0)
        self.state["mode"] = "auto"
        self.sup.tick(self.b, 95.0)
        self.state["mode"] = "silent"
        self.sup.tick(self.b, 95.0)
        self.assertEqual(len(self.b.duties), 2)
        self.assertEqual(self.b.duties[0], self.b.duties[1])

    def test_direct_modes_are_left_alone(self):
        self.state["mode"] = "turbo"
        self.sup.tick(self.b, 95.0)
        self.assertEqual(self.b.duties, [])
        self.assertEqual(self.b.autos, 0)

    def test_ceiling_releases_the_fans_once(self):
        self.b.temps["cpu"] = 96.0
        self.sup.tick(self.b, 95.0)
        self.sup.tick(self.b, 95.0)
        self.assertEqual(self.b.autos, 1)
        self.assertEqual(self.b.duties, [])

    def test_ceiling_rearms_and_drives_again(self):
        self.sup.tick(self.b, 95.0)
        self.b.temps["cpu"] = 96.0
        self.sup.tick(self.b, 95.0)
        self.assertEqual(self.b.autos, 1)
        self.b.temps["cpu"] = 88.0
        self.sup.tick(self.b, 95.0)      # back under the ceiling: re-arm
        self.sup.tick(self.b, 95.0)      # and drive the curve again
        self.assertEqual(len(self.b.duties), 2)

    def test_floor_overrides_a_curve_that_idles_a_hot_fan(self):
        self.state["mode"] = "custom"
        self.state["curve"] = [[40, 5], [95, 10]]
        self.b.temps["cpu"] = 90.0
        self.sup.tick(self.b, 95.0)
        self.assertGreaterEqual(self.b.duties[0][0], g5fan.FLOOR_PCT)

    def test_gpu_temperature_drives_the_gpu_fan(self):
        self.b.temps["cpu"] = 45.0
        self.b.temps["gpu"] = 60.0
        self.sup.tick(self.b, 95.0)
        cpu, gpu = self.b.duties[0]
        self.assertEqual(cpu, g5fan.duty_for(g5fan.PROFILES["silent"], 45))
        self.assertEqual(gpu, g5fan.duty_for(g5fan.PROFILES["silent"], 60))

    # -- the snapshot the panel reads ---------------------------------

    def test_a_tick_publishes_what_it_saw(self):
        """The panel's numbers come from here, so every tick must publish —
        including the ones that changed nothing, whose age is how a reader
        tells a live daemon from one that died."""
        self.sup.tick(self.b, 95.0)
        data = g5fan.read_snapshot()
        self.assertIsNotNone(data)
        self.assertEqual(data["mode"], "silent")
        self.assertEqual(data["cpu_temp_c"], 50)
        self.assertEqual(data["gpu_temp_c"], 45)
        self.assertEqual(data["fans"][0]["label"], "CPU")
        self.assertEqual(data["fans"][0]["duty_pct"], 35)
        self.assertEqual(data["fans"][0]["rpm"], 2218)
        self.assertTrue(data["daemon"])          # it is running: it just wrote
        self.assertFalse(data["stale"])

    def test_a_tick_that_changes_nothing_still_publishes(self):
        self.state["mode"] = "turbo"      # a direct mode: nothing to drive
        self.sup.tick(self.b, 95.0)
        self.sup.tick(self.b, 95.0)
        self.assertEqual(self.b.duties, [])
        self.assertIsNotNone(g5fan.read_snapshot())

    def test_a_broken_snapshot_path_does_not_kill_the_tick(self):
        """Losing the panel's numbers must never cost the fans their curve."""
        def explode(_data):
            raise RuntimeError("disk on fire")

        _patch(self, publish_snapshot=explode)
        self.sup.tick(self.b, 95.0)
        self.assertEqual(len(self.b.duties), 1)


class ReadingBackend(RecordingBackend):
    """The same stand-in, but with a kernel driver behind it."""
    root = "/sys/bus/acpi/devices/CLV0001:00"


class TestStatusContract(unittest.TestCase):
    """`g5fan status --json` is the panel's only way of looking at the fans,
    so its field names are a contract: they are mirrored by `RawFanStatus` in
    gui/src-tauri/src/lib.rs, and its `parses_a_fan_status_snapshot` test does
    the other half. A rename that only happens on one side makes the panel
    draw an empty fan list instead of failing, which is exactly the kind of
    bug nobody notices."""

    KEYS = {
        "mode", "backend", "driver", "manual_duty", "curve", "custom_curve",
        "presets", "fans", "cpu_temp_c", "gpu_temp_c", "ceiling_c", "daemon",
        "generated",
    }
    FAN_KEYS = {"label", "duty_pct", "rpm", "tacho"}

    def setUp(self):
        self.state = {"mode": "silent", "duty": None, "curve": None}
        _patch(self,
               load_state=lambda: dict(self.state),
               cpu_ceiling_c=lambda: 100.0,
               unit_active=lambda: True)
        self.b = ReadingBackend()

    def test_key_set(self):
        self.assertEqual(set(g5fan.status_data(self.b)), self.KEYS)
        self.assertEqual(set(g5fan.status_data(self.b)["fans"][0]), self.FAN_KEYS)

    def test_readings(self):
        data = g5fan.status_data(self.b)
        self.assertEqual(data["mode"], "silent")
        self.assertEqual(data["backend"], "test")
        self.assertTrue(data["driver"])           # ReadingBackend has a root
        self.assertIsNone(data["manual_duty"])
        self.assertEqual(data["ceiling_c"], 100.0)
        self.assertEqual([f["duty_pct"] for f in data["fans"]], [35, 35])
        self.assertEqual([f["rpm"] for f in data["fans"]], [2218, 2113])
        self.assertEqual(data["curve"], [list(p) for p in g5fan.PROFILES["silent"]])

    def test_the_panel_gets_every_preset(self):
        """The panel must not carry its own copy of the curves."""
        presets = g5fan.status_data(self.b)["presets"]
        self.assertEqual(set(presets), set(g5fan.CURVE_MODES))
        for name, points in g5fan.PROFILES.items():
            self.assertEqual(presets[name], [list(p) for p in points])

    def test_the_saved_custom_curve_is_reported_verbatim(self):
        self.state["mode"] = "custom"
        self.state["curve"] = [[40, 10], [90, 100]]
        data = g5fan.status_data(self.b)
        self.assertEqual(data["custom_curve"], [[40, 10], [90, 100]])
        self.assertEqual(data["curve"], [[40, 10], [90, 100]])

    def test_only_direct_driven_modes_have_a_curve(self):
        self.state["mode"] = "manual"
        self.state["duty"] = 60
        data = g5fan.status_data(self.b)
        self.assertIsNone(data["curve"])
        self.assertEqual(data["manual_duty"], 60)

    def test_it_is_json_serialisable(self):
        data = g5fan.status_data(self.b)
        self.assertEqual(json.loads(json.dumps(data)), data)

    def test_manual_duty_is_only_reported_for_manual(self):
        self.state["mode"] = "turbo"
        self.state["duty"] = 60           # stale leftover from a manual mode
        self.assertIsNone(g5fan.status_data(self.b)["manual_duty"])


class TestSnapshot(unittest.TestCase):
    """The published reading, which is what makes looking at a fan
    unprivileged — and its age, which is how a reader knows the daemon died."""

    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="g5fan-snap-")
        self.addCleanup(shutil.rmtree, self.dir, True)
        self.path = os.path.join(self.dir, "status.json")
        _patch(self, SNAPSHOT_PATH=self.path)

    def _publish(self, **overrides):
        data = {
            "mode": "silent", "backend": "kernel", "driver": True,
            "manual_duty": None, "curve": [[45, 15], [92, 100]],
            "custom_curve": [[50, 25], [95, 100]], "presets": {},
            "fans": [{"label": "CPU", "duty_pct": 20, "rpm": 2218,
                      "tacho": 972}],
            "cpu_temp_c": 52, "gpu_temp_c": 44, "ceiling_c": 100.0,
            "daemon": True, "generated": time.time(),
        }
        data.update(overrides)
        g5fan.publish_snapshot(data)
        return data

    def test_round_trip(self):
        sent = self._publish()
        got = g5fan.read_snapshot()
        self.assertEqual({k: got[k] for k in sent}, sent)
        self.assertFalse(got["stale"])
        self.assertLess(got["age_s"], 5)

    def test_the_panel_can_read_it(self):
        """Without this mode the whole point is lost: an unprivileged reader
        must be able to open it."""
        self._publish()
        self.assertEqual(oct(os.stat(self.path).st_mode & 0o777), "0o644")

    def test_a_stale_snapshot_is_flagged(self):
        self._publish(generated=time.time() - g5fan.SNAPSHOT_MAX_AGE - 1)
        self.assertTrue(g5fan.read_snapshot()["stale"])

    def test_no_snapshot(self):
        self.assertIsNone(g5fan.read_snapshot())

    def test_a_truncated_snapshot_is_ignored(self):
        with open(self.path, "w") as f:
            f.write('{"mode": "silent"')
        self.assertIsNone(g5fan.read_snapshot())

    def test_a_snapshot_without_a_generation_time_is_ignored(self):
        self._publish(generated="eleven")
        self.assertIsNone(g5fan.read_snapshot())

    def test_a_snapshot_without_fans_is_ignored(self):
        self._publish(fans=[])
        self.assertIsNone(g5fan.read_snapshot())

    def test_publishing_creates_the_directory(self):
        _patch(self, SNAPSHOT_PATH=os.path.join(self.dir, "deep", "a", "status.json"))
        self._publish()
        self.assertIsNotNone(g5fan.read_snapshot())


if __name__ == "__main__":
    unittest.main()
