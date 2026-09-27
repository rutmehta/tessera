# Review hands-on validation

Source checkpoint: a54044fbefdbae56229f74053402d513c92674ef.
Dev-test bundle: /Volumes/betterSSD/tessera-validation/ux02a/a54044f/Tessera-UX02a-UniqueExec.app.
Identity: dev.tessera.validation.ux02a.uniqueexec, process36425, executableTesseraUX02a.
Packaging manifest: /Volumes/betterSSD/tessera-validation/ux02a/a54044f/manifest-unique.json.
This uses pre-export-integration FFI and is not release or combined-main acceptance.

Luna agent review_hands_on exercised actual UI via CUA on isolated generatedJPEGs
and app-support-unique; no user catalog was used. Observed outcomes:
- Selected05-Review-return-target.jpg, Library index1of6. D opened PhotoEdit on
  that target; Back to Library restored target/index.
- AutoEdit dialog initially offered Whole shoot6; tester explicitly chose Selection1.
  Scripted planner completed one edit and produced one review entry. Other five
  fixtures showed no unexpected changes. Default batch scope remains UX03 follow-up.
- Review displayed the saved preview, selected row, rationale and scoped actions.
  Edit photo then Back to Review restored the same target.
- Typing d in the redo instruction field kept d in the field, with no edit shortcut.
  Draft was canceled without submitting a redo operation.
- Accept and next marked the sole target Accepted, summary0to review/1accepted,
  style profile1sample. No next-row advance claim is possible for a one-entry queue.

These are observed AX/screenshot outcomes, not timings. Missing-file, multi-row
advance, actual redo/revert and undo-pixel flows were not exercised hands-on in
this bounded run; source tests cover their own documented assertions separately.
CUA showed screenshots inline; no local screenshot files were saved by this agent.

The first dev-test bundle with duplicate process basenameTessera could not bind
(CUA timeouts). Main thread sample was idle. The uniquely named executable variant
bound immediately and exposed all six fixtures. This controlled package variant
resolved the access blocker; no product source fix or definitive CUA cause claimed.
