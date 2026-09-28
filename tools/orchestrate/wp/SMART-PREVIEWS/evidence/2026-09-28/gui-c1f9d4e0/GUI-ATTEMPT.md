# GUI attempt — startup blocked, acceptance incomplete

Only owned final08 app launched, PID70776, exact --app-dir profile --folder photos arguments. Initial packaging error: copyfile omitted executable mode; corrected only owned executable0644->0755, changed reproducible packager to copy2, codesign deep/strict passed again. No executable bytes changed by this correction.

Fresh native CUA inventory showed owned app running. getApp using exact bundle identifier and full path each timed out (-10005). Later retry returned Cocoa256 with underlying AppleEvent -1712 timeout. No accessibility tree/screenshot of the owned window was obtained. No Smart Preview GUI operation was performed; separate-process offline restart, reconnect, sync and GUI export remain UNTESTED.

Read-only process sample retained as startup-sample.txt: main thread in TesseraApp.init -> AppModel.init -> ExportController.init -> NSUserDefaults/CFPreferences attachSandboxExtensionToken/create parent -> kernel open throughout sample. This establishes startup blockage, not its cause or a permission decision. No protected prompt, other app or global setting was manipulated.

App remains PID70776 at time of report because native CUA cannot obtain the app for ordinary Quit. Root notified; no forced termination attempted. Fixture directory has not been renamed or disconnected.
