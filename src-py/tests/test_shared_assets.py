"""Checks on the things the two implementations share.

`ui/`, `assets/` and `settings.json` are used by both the Python and the Rust
build, and each failure guarded here has the same nasty shape: nothing
complains until the app is actually launched, which for this app means at a gig.

- A template the automation loads can be renamed or moved and the Rust build
  will still compile, because it embeds its own copy with `include_bytes!`.
- A file missing from the PyInstaller or Nuitka command in cmds.md still
  produces a working-looking exe; cmds.md says so itself — miss one and the
  build only fails when you launch it.
- `ui/app.slint` is compiled by build.rs for Rust and loaded at runtime by
  Python, so a change that suits one can break the other with nothing in
  between to notice.
"""

import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from utils import resource_path  # noqa: E402

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


# ---------------------------------------------------------
# Assets the automation loads by path
# ---------------------------------------------------------


def _resource_paths_in(module_name):
    """Every literal passed to resource_path() in one src-py module."""
    source_path = os.path.join(REPO_ROOT, "src-py", module_name)
    with open(source_path, encoding="utf-8") as f:
        source = f.read()
    return re.findall(r"resource_path\(\s*[\"']([^\"']+)[\"']\s*\)", source)


def test_every_asset_the_defender_automation_loads_exists():
    """A renamed template leaves the Rust build compiling happily, because it
    embeds its own copy, while the Python build silently stops matching
    anything and every Defender click lands on nothing.
    """
    referenced = _resource_paths_in("switch_defender.py")
    assert referenced, "no resource_path calls found — has the module moved?"

    missing = [p for p in referenced if not os.path.isfile(resource_path(p))]
    assert not missing, f"switch_defender.py loads files that are not there: {missing}"


def test_every_asset_the_launcher_loads_exists():
    missing = [
        p
        for p in _resource_paths_in("gig_launcher.py")
        if not os.path.isfile(resource_path(p))
    ]
    assert not missing, f"gig_launcher.py loads files that are not there: {missing}"


# ---------------------------------------------------------
# Files the packaging commands bundle
# ---------------------------------------------------------


def _packaged_sources():
    """Every source path the build commands in cmds.md bundle into the exe."""
    with open(os.path.join(REPO_ROOT, "cmds.md"), encoding="utf-8") as f:
        cmds = f.read()
    pyinstaller = re.findall(r'--add-data "([^";]+);', cmds)
    nuitka = re.findall(r'--include-data-files="([^"=]+)=', cmds)
    return pyinstaller, nuitka


def test_every_file_pyinstaller_bundles_exists():
    """cmds.md warns that a missed --add-data entry only fails when the exe is
    launched. This is that warning, enforced.
    """
    pyinstaller, _ = _packaged_sources()
    assert pyinstaller, "no --add-data entries found — has the command changed?"

    missing = [p for p in pyinstaller if not os.path.isfile(os.path.join(REPO_ROOT, p))]
    assert not missing, f"PyInstaller bundles files that are not there: {missing}"


def test_every_file_nuitka_bundles_exists():
    """The Nuitka recipe is the documented alternative, so it has to keep step
    with the PyInstaller one rather than quietly rotting.
    """
    _, nuitka = _packaged_sources()
    assert nuitka, "no --include-data-files entries found — has the command changed?"

    missing = [p for p in nuitka if not os.path.isfile(os.path.join(REPO_ROOT, p))]
    assert not missing, f"Nuitka bundles files that are not there: {missing}"


def test_both_packaging_commands_bundle_the_same_files():
    """The two recipes drifting apart is how one build ends up missing an asset
    the other has.
    """
    pyinstaller, nuitka = _packaged_sources()
    assert set(pyinstaller) == set(nuitka), (
        "the PyInstaller and Nuitka commands in cmds.md no longer bundle the "
        f"same files; only in PyInstaller: {sorted(set(pyinstaller) - set(nuitka))}, "
        f"only in Nuitka: {sorted(set(nuitka) - set(pyinstaller))}"
    )


# ---------------------------------------------------------
# The shared Slint UI
# ---------------------------------------------------------


def test_the_shared_slint_ui_compiles_for_the_python_build():
    """build.rs compiles ui/app.slint for Rust, so a Rust-only change can be
    merged green while leaving the Python build unable to start. Loading it
    here is the only thing that notices.
    """
    import slint

    components = slint.load_file(resource_path("ui/app.slint"))

    assert hasattr(components, "MainWindow"), "ui/app.slint no longer exports MainWindow"
    assert hasattr(
        components, "SplashWindow"
    ), "ui/app.slint no longer re-exports SplashWindow"


def test_the_advanced_settings_properties_exist_on_both_sides():
    """The Advanced fields are Rust-only in behaviour, but they live in the
    shared UI file, so Python has to keep being able to see them — that is what
    makes a later Python implementation a wiring job rather than a rewrite.
    """
    import slint

    window = slint.load_file(resource_path("ui/app.slint")).MainWindow()

    expected = [
        "defender_deactivation_state_image",
        "defender_activation_state_image",
        "defender_realtime_label_image",
        "defender_manage_settings_image",
        "executable_gig_performer",
        "executable_open_stage_control",
    ]
    missing = [name for name in expected if not hasattr(window, name)]
    assert not missing, f"ui/app.slint no longer exposes: {missing}"

    assert window.advanced_expanded is False, "the Advanced section should start collapsed"
