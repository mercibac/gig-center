- compilation with pyinstaller (onefile)
  same --with / python -m note as the nuitka commands below
  output: dist/Gig Control Center.exe
uv run --with pyinstaller python -m PyInstaller --noconfirm --onefile --windowed --name "Gig Control Center" --icon="assets/icon.ico" --add-data "ui/app.slint;./ui" --add-data "assets/osc_icon.png;./assets" --add-data "assets/icon.ico;./assets" --add-data "assets/defender_activate_icon.png;./assets" --add-data "assets/defender_protection_icon.png;./assets" --add-data "assets/defender_realtime_icon.png;./assets" --add-data "assets/defender_settings_icon.png;./assets" --add-data "settings.json;." src-py/main.py


- compilation of slint version with nuitka
  nuitka is not a project dependency, so --with pulls it in for the run, and
  python -m is needed because uv does not expose its console script.
  In a non-interactive shell add --disable-ccache, or Nuitka stops to ask
  whether it may download ccache.
  output: main.dist/Gig Control Center.exe
uv run --with nuitka python -m nuitka --standalone --windows-disable-console --output-filename="Gig Control Center.exe" --windows-icon-from-ico="assets/icon.ico" --include-data-files="ui/app.slint=ui/app.slint" --include-data-files="assets/icon.ico=assets/icon.ico" --include-data-files="settings.json=settings.json" --include-data-files="assets/osc_icon.png=assets/osc_icon.png" --include-data-files="assets/defender_activate_icon.png=assets/defender_activate_icon.png" --include-data-files="assets/defender_protection_icon.png=assets/defender_protection_icon.png" --include-data-files="assets/defender_realtime_icon.png=assets/defender_realtime_icon.png" --include-data-files="assets/defender_settings_icon.png=assets/defender_settings_icon.png" src-py/main.py

- compilation standalone with nuitka and upx for compression
  same invocation notes as above; --upx-binary still needs a real upx path
  output: main.dist/Gig Control Center.exe
uv run --with nuitka python -m nuitka --standalone --follow-imports --deployment --lto=yes --windows-disable-console --output-filename="Gig Control Center.exe" --windows-icon-from-ico="assets/icon.ico" --windows-company-name="Your Company" --windows-product-name="Gig Control Center" --windows-file-version="1.0.0.0" --windows-product-version="1.0.0.0" --include-data-files="ui/app.slint=ui/app.slint" --include-data-files="assets/icon.ico=assets/icon.ico" --include-data-files="settings.json=settings.json" --include-data-dir="assets=assets" --plugin-enable=upx --upx-binary="C:\path\to\upx.exe" --remove-output --no-pyi-file src-py/main.py
