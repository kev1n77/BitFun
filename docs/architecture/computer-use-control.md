# Computer Use control-session SPEC

Status: the October 2026 migration implements permissions, user-selected App
sharing and control grants, coordinate mapping, virtual cursor feedback and
platform-scoped background input. Validation includes native macOS Tool/input/
capture checks, Linux GTK/AT-SPI execution and Windows native-module
cross-compilation. Windows native execution, GNOME Wayland portal execution and
remote end-to-end control remain acceptance gaps. Compilation is not native
acceptance, and background support is limited to each provider's declared routes.

## Product contract

Computer Use controls the GUI on the machine that owns the selected provider.
An active session identifies its target and mode, provides persistent feedback,
and can be stopped. Capturing an obscured window must return that window's pixels,
never the covering application's content or an unsolicited desktop capture.

Background mode must not raise a window, change the human's keyboard focus, switch
Spaces/desktops, or silently use global seat input. A backend's inability to meet
that contract must be visible. Foreground takeover requires explicit user
permission; approval of a message's content and recipient does not authorize it.
A generated agent plan cannot widen the original user's authorization.

The operating system owns privacy/sharing indicators. OpenBitFun neither imitates
macOS's Sharing menu on other systems nor suppresses native capture borders.
Product feedback adds target, mode, action, preview and Stop without claiming to
replace system consent or system Stop Sharing.

## Platform architecture

| Platform | Authorized observation | Input | System integration |
|---|---|---|---|
| macOS | Persistent ScreenCaptureKit `SCStream`, exact single-window filter | AX semantic actions; directed application events where supported; global input only with foreground authorization | System sharing integration and native stop; nonactivating AppKit feedback |
| Windows | Persistent Windows.Graphics.Capture window session | UI Automation patterns and supported targeted window messages; `SendInput` requires foreground authorization | WGC capture border and Closed event; product preview/Stop |
| Ubuntu Wayland | RemoteDesktop and ScreenCast portals sharing one session and its authorized PipeWire FD/stream | AT-SPI semantic actions; portal Notify seat input only in explicit foreground mode | Portal consent, compositor sharing UI and Session.Closed |
| Ubuntu X11 | New explicit control sessions use the same portal path | AT-SPI; missing portal capabilities fail explicitly | Product status/Stop; no claim of Wayland isolation for legacy X11 calls |

The control snapshot’s `capabilities` lists backend routes, such as semantic
input, directed keyboard/scroll or foreground seat input. It is not a promise
that every control supports a route; a background capability does not authorize
raw pointer gestures. Window capture, semantic input and arbitrary pointer input
are distinct capabilities. Portal selection does not identify a PID or authorize an unrelated
AT-SPI tree. Wayland seat input cannot provide isolated background coordinates.
Games, custom renderers, protected content, minimized/hidden windows and other
Spaces require explicit capability/error handling; no backend promises universal
background input.

## Ownership and execution

Portable DTOs and `AppInputAction` live in `tool-contracts`; the host port lives in
Core. Desktop owns native resources and platform adapters. Assembly must not own
OS APIs, native focus workarounds or concrete capture implementations.

The shared action catalog and structural target validation live in
`tool-contracts`. Desktop's `control_session/admission.rs` is a deterministic
owner/mode/lease state machine; its facade alone performs native cleanup and
carries tokens across workers. `desktop_host/observation.rs` coordinates native
reads, while `observation_cache.rs` stores bounded references without owning input
permission. `computer_use_geometry` adapts the independent `computer-use-rust`
project's image/content/surface mapping for all three platforms. The reference
project's macOS-only capture/input and standalone MCP/session shell are not a
replacement for the existing Windows, Linux or product runtime providers.

### Reference implementation migration

The five requested areas are adapted from `computer-use-rust` into the existing
product owners. The reference repository is development evidence, never a
runtime, build, test or filesystem dependency of OpenBitFun.

| Area | Reference source | OpenBitFun owner and adaptation |
|---|---|---|
| Permission query/request | `src/native/permissions.rs`, native permission probes | `computer_use_permissions` contracts, Desktop `permissions.rs`, `ComputerUsePermissions.tsx`; four separate read-only probes and explicit request receipts; Windows capability/UIPI facts and Linux AT-SPI/session consent replace macOS-specific assumptions |
| App sharing and control grants | `src/sharing.rs`, `src/grants.rs`, sharing controls in `src/gui.rs` | Admission state machine plus Desktop `sharing.rs`, `shared_preview.rs`, `process_identity.rs`, `grants.rs`, `share_grants.rs`; user-selected native window, observe first, separate revocable grant, executable-bound remembered preference, manual preview gestures |
| Coordinate conversion | `CoordinateMap` in `src/sharing.rs` | Portable `CaptureGeometry` and Web `previewGeometry.ts`; image/content/global/preview round trips, aspect fit, negative origins, independent X/Y scales, strict padding rejection and immutable screenshot identity |
| Virtual mouse | `src/cursor.rs`, GUI cursor rendering | Web `VirtualCursor.ts` ports the deterministic physical model, rendered by `PreviewCursor.tsx`; 63 Rust-generated reference samples cover movement, retargeting, immediate placement, resize and visibility. Existing passive native overlays remain platform adapters |
| Background control | Native directed/semantic input and desktop-state comparison | Existing AX/UIA/AT-SPI and directed-input providers plus `background_guard.rs`; focus/pointer/window-order evidence, explicit unsupported routes, revocation on observed interference, no restore or replay. The reference's implicit Command modifier is not copied because it changes the requested click |

### Permissions and user sharing

Opening settings, checking status or reading AX never requests OS permission.
The user selects each request explicitly. Receipts distinguish a request API
invocation from the current grant and from observing a visible OS dialog. Input
Monitoring is optional. Windows does not invent a macOS-style grant; protected
desktops, capture policy and target integrity still apply. Linux's portal grant
belongs to one selected session; AT-SPI availability does not grant seat input.

`start_control` and implicit tool admission return `awaiting_share` / a clear
`SHARE_REQUIRED` result until the user selects a target in the executing host's
control card. Discovery returns opaque choices bound to the process lifetime,
not authority derived from a title or a client-supplied PID. Starting the selected
share validates the live process and exact window, obtains real pixels or the
supported AT-SPI tree, then exposes observation. Input requires a separate user
grant for the selected background or foreground mode. A tool cannot select a
different window, grant itself input or widen the mode.

Remembering a grant is optional and never starts a share or selects a window.
The preference binds the canonical executable path, full executable SHA-256,
bundle identity when available and input mode; process start identity and file
metadata are checked around hashing. An updated executable requires a new
grant. The versioned bounded file preserves unknown fields and unreadable user
data; writes are atomic, privately permissioned and protected against concurrent
changes. Forgetting an active matching preference revokes input before disk IO.
Portal streams have no verified executable identity and remain session-only.

Manual preview input joins the same exclusive action lease as the Agent. Pointer
gestures retain their screenshot until release; resizing or revocation cancels
them. Click/right click/double click, drag, scroll, text and key input use the
native provider. The preview cannot pass Agent-owned AX indices. Input errors
revoke the grant and are never automatically replayed. Linux background sharing
shows the AT-SPI tree and exposes semantic Agent actions; it does not fabricate
pixels or coordinate input for an unrelated portal surface.

The app-input pipeline has one execution owner:

```text
single app_* call ─┐
                  ├─ validate → typed input program → dispatch_app_input per step
app_batch steps ──┘                                    ↓
                                            one final observation
                                                      ↓
                                            receipts + pixels/AX
```

`computer_use_program` owns ordered dispatch, receipts, cancellation checks and
final observation. Single-action aliases compile to the same typed program as
`app_batch`. The former four host methods combining click/text/scroll/key input
with a snapshot have been removed from the internal port. There is no separate
single-action recovery executor that can repeat a mutation or take redundant
intermediate observations.

A program selects one application and cannot switch target or mode between
steps. Click, text, key chord, scroll, drag and wait steps are all parsed and
validated before any input is dispatched. Structural checks include image IDs,
nonnegative image coordinates, grid dimensions/indices and nonempty OCR targets,
including text/scroll focus targets in later steps. Legacy shorthand coordinates
and node indices use checked integer conversion rather than wrapping onto a
different control. Native providers still validate live bounds and identity.
`dispatch_app_input` performs input
only. A known sequence can avoid intermediate model/AX/OCR/capture work; unknown
search results still require an observation before choosing a result. macOS and
Windows clicks have no implicit per-step settle sleep; explicit `wait_ms_after`
is preserved. A program takes its final observation through the capture provider
instead of multiplying a fixed delay by its step count.

For an observed search field whose Return-to-search behavior is already known,
a program can combine `app_type_text` with that field as `focus`, followed by
`app_key_chord` with `["return"]`. Inspect the resulting search results before
choosing one. Focus-and-type alone is already one input step. Model-facing schemas
retain this concrete combination for both image and node targets; a batch changes
neither native delivery capability nor authorization.

The executor checks active owner/generation/target scope before each step and
before final observation. Cancellation interrupts side-effect-free waits. Native
input returns its delivery result so partial delivery is not hidden by dropping
a future. Remaining steps are not run after failure or cancellation. Native macOS gestures also release any presses left unfinished by a failed or cancelled action, so a partial chord or drag cannot leave modifiers or mouse buttons held for the next action. An
observation completing after stop, rebinding or generation change is discarded.

Receipts distinguish attempted steps, submitted steps, the failing step and
observation failure. A native error may follow partial submission. Neither that
error, a missing screenshot nor an unchanged AX digest authorizes replay.
Submission means accepted by the delivery path, not verified application success;
completion must be established by the resulting application state. The evidence
helper fingerprints every encoded byte, but hashes cannot measure changed image
area or establish completion. Its legacy loop API makes no loop claim from
repeated action names, mouse usage or identical frames. Automatic retry policy
is restricted to transient read failures; mutation errors never authorize replay.

Main agents can use ComputerUse directly when the host and tool configuration
allow it. The ComputerUse subagent is optional. Delegated execution keeps
`original_user_context` separate from `agent_plan`, with message provenance;
missing original context never becomes foreground permission. Browser automation
is preferred for attached web content, but a browser process name is not an input
authorization boundary for native chrome, dialogs or unattached windows.

## Session lifecycle and revocation

The user-facing resource lifecycle is
`idle → awaiting_share → active(observe) → active(authorized mode) → stopped|failed`,
with internal starting/cleanup transitions. One Runtime session owns the GUI
resource. Competing ownership returns `control_busy`; it does not create a
second permission owner or Agent loop.

An explicit tool start requests a mode; the user's share selection and grant
establish authority. Discovery alone does not create capture. Legacy wire fields
remain readable but do not bypass the new host's user-selection gate. Linux
portal capture binds the consented surface separately from semantic application
identity. Stop/status are idempotent. A stopped generation requires an explicit
restart, not a screenshot-driven resurrection. Revoking control preserves the
shared observation and increments the authorization revision immediately; stale
workers lose input authority and held native inputs are released.

Stop revokes admission and advances generation before cancelling native work,
releasing owned pressed keys/buttons, stopping capture and hiding feedback.
Native Closed/Stop Sharing, cancellation, permission revocation and target loss
use the same owner path. Generation/sequence tokens cross blocking/native worker
boundaries; observation leases never carry input authority. Unknown actions fail
before admission. An unrelated executor cannot observe or rebind an in-flight
action without carrying its original token. Target revisions distinguish every bind, including A → B → A
in one generation, and native stop callbacks carry the stream's original revision.
Delayed work cannot borrow a later action's admission. Startup and
cleanup epochs prevent late callbacks from resurrecting or clearing newer state.
Native cleanup is serialized and rechecks its epoch after acquiring the cleanup
lock; a delayed cleanup job cannot tear down a restarted session.
Already submitted external effects cannot be rolled back.

## Observation, coordinates and model transport

A screenshot returns the authorized native capture's bytes, screenshot ID,
image/native dimensions and coordinate geometry. It does not perform a four-way
display search or require a confirmation crop. Screenshot admission requires a
supported control provider and a bound capture target. Legacy crop, quadrant,
reset-navigation and window hints remain accepted with an explicit ignored-field
notice; they do not select a new surface or grant authorization. Legacy DTOs
remain readable, without advertising their obsolete navigation workflow.

Image coordinates are meaningful only with the corresponding `screenshot_id`,
`image_content_rect` and `image_global_bounds`. Apply scaling once. Reject unknown
IDs, padding, out-of-range points, target changes and invalid geometry. No
clamping can redirect a bad coordinate onto an edge control. macOS and Windows
query live native bounds and can translate a retained image's
coordinates for pure window movement when the image projection and size remain
valid; resizing or changed projection requires fresh observation. Retina, mixed
DPI, negative origins and cross-monitor movement are acceptance cases.

Screenshot geometry is scoped by control generation and target revision and
retained in a bounded history of 16 images; pixels are not retained in that cache.
Stop, restart and rebind invalidate it. Preview frames carry the same revision,
and controllers reject stale responses even when the target string matches.

Observed node indices refer to retained native identities from that observation.
`observation_id` identifies the node address space independently of the content
`digest`. Every application snapshot creates a new identity, invalidates derived
interactive/visual views, and stamps all returned views with that identity.
New callers echo it on node/index input; hosts reject unknown/stale explicit IDs
before dispatch. Legacy callers can omit it, and older payloads default it to
absent. Native AX/UIA/AT-SPI caches independently validate generation and target
revision and release retained observations on Stop. macOS node lookups retain
their own guard before unlocking the cache, so concurrent cleanup cannot free an
in-flight reference. Do not rebuild a tree and
reinterpret an old index. Bound-window input validates
the element's actual owning window, including same-PID foreign-window rejection.
Application-level menu exceptions must be explicit, not a general unknown-window
escape hatch.

`describe_screen` observes the explicit or bound target. The human foreground app
and physical pointer are separate metadata. Window chrome alone is incomplete AX
content, not successful reading of a document or conversation. Native OCR uses
the authorized pixels. Explicit app AX reads can survive classified surface/frame
or screen-capture-permission failures, marked `capture_preparation_error`,
`capture_status: unavailable` and `control_target_available: false`. Such reads
create no binding and attach no prior target's pixels. Lock, stop, ownership,
generation, revocation and unknown errors cannot use that fallback.

A text-only screenshot request performs a real text observation. Tool results
preserve structured identities, AX/OCR facts, errors and geometry in the model's
actual result channel; a short summary is not a replacement. Image attachments
retain their bytes and metadata through supported OpenAI-compatible, Responses,
Anthropic and Gemini transports. Model vision capability and transport support
are separate gates. Persisted images remain available to capable later consumers.

Post-input observation is fresh with respect to the capture provider's guarantee.
macOS marks a host-clock input barrier and waits for a later complete or idle
ScreenCaptureKit sample. Idle can establish unchanged pixels; resize still needs
a valid complete frame. This proves capture timing, not completion of asynchronous
application work. Windows drains WGC frames to the newest available frame; it
does not claim an equivalent input-clock barrier.

Windows input preparation validates and reuses the live WGC session without
consuming or encoding a frame. Only an actual observation requests pixels. WGC BGRA converts directly to RGBA
and the host encodes JPEG; there is no intermediate PNG, GDI/PrintWindow path or
desktop capture fallback. Preview encoding supplies its known dimensions rather
than decoding its own JPEG. Unchanged legitimate dark frames are preserved.
Explicit action waits subscribe to session changes; Stop, revocation or target
replacement interrupts them immediately, including waits between batch steps.

## Native input rules

Background actions capture desktop witnesses before and after submission.
macOS checks foreground identity plus its activation epoch, the physical pointer
and relative order of existing windows; Windows checks foreground HWND, pointer
and window order. Linux can inspect active AT-SPI top-level windows but cannot
observe compositor cursor/stacking, which remain unknown in the receipt. A
missing preflight witness sends no input. Changed or unavailable post-action
evidence revokes the session without focus restoration or retry. Concurrent
human/system input can also change these witnesses, so an interference error
does not blame the application or claim that already submitted effects were
undone. Boundary samples cannot prove all transient or later application behavior.

### macOS

Retain the selected content window across observations. A sharing-indicator
window appearing earlier in WindowServer order must not replace it. Conflicting
explicit window selection fails; validate a replacement before releasing a live
capture. AX focused-window observation resolves the actual bound window, not an
unrelated Sharing dialog. Missing exact identity is an error.

For one unmodified left ImageXy click, native application-root AX hit testing may
select an explicitly pressable button, checkbox, radio button or link. The hit
must belong to the exact bound window and contain the point. Do not rank nearby
rectangles, climb ancestors or replace double/right/modifier clicks with AXPress.
Only explicit ActionUnsupported permits a different delivery path; unknown AX
outcomes stop without a duplicate click.

Text/key `focus` uses retained AX identity or native hit testing and semantic
focus for text controls. Other eligible targets use the exact-window directed
focus route described below. Unknown focus outcomes must not type into an old
field or silently switch to global foreground input.
For explicit text-control focus, typing first uses one `AXSelectedText` mutation
at the retained target: replace the selected range or insert at the caret.
Preflight-confirmed unsupported semantic writing may use directed Unicode input;
an unknown write outcome stops without resubmission. An already focused control
is not focused again, preserving its selection. Semantic Unicode field input
passed the dedicated production Tool fixture; support for other applications
still depends on their native accessibility implementation.

Explicit foreground mode may use its authorized pointer-focus path. An unknown
AX outcome never implies successful focus. Scroll `focus` is only a coordinate
anchor: resolve it, update virtual pointer position, then scroll; never click the
control under the anchor.

Directed pointer events use one `CGEventPostToPid` route and exact window/local
coordinates. The existing dynamically resolved `CGEventSetWindowLocation`
routing dependency is checked; its absence is an explicit error. Authenticated
event delivery obtains the event record through `SLEventGetEventRecord` and
copies it into an owned, aligned 248-byte record. It no longer guesses offsets
inside an opaque CGEvent. The getter has a local no-input test; that test does
not establish delivery. The unused temporary foreground-menu activation helper
is removed. No implicit
Command modifier, global foreground activation or human-cursor warp is a substitute.
The directed mouse source is `CombinedSessionState`, with modifier flags set
explicitly to the requested modifiers. A private mouse source itself caused
AppKit to raise inactive windows in the strict fixture; changing only that source
is therefore part of the routing contract, not a reason to add Command. Keyboard
events retain a private source. Event field 58 is set to 1 for window routing; it
is not a click-group identifier and must not be repurposed as one. These event
fields are native adapter details, not application-specific rules.

#### Target-local focus lifecycle

`macos_input_focus` prepares the target application's internal active/focus state
without making that process the human desktop's frontmost application. It
validates the exact window ID, current geometry and capture action lease. A
window-addressed down/up focus pair is placed outside the content frame at local
`(-8,-8)`; it must not hit a content control. The requested gesture then uses its
observed target coordinates and original modifiers. There is no implicit Command,
global foreground switch, window raise/restore or human-cursor warp.

The preparation cache includes generation, PID, window ID, human foreground
identity and activation epoch, so preparation is not repeated for every key or
batch step while stale activation state cannot be reused. There is no app-name
rule or guessed content-control coordinate. Preparation does not depend on an AX
key-state acknowledgement, which normal AppKit windows can omit; fixtures inspect
their own `NSWindow.keyWindow` directly.

Cleanup validates process-serial identity before touching the target, and skips deactivation if the user has made that target the actual
foreground application. It cannot deactivate a reused PID or reset another
application's focus. The outside-frame focus pair and cleanup belong to the
control lifecycle, not to a model-generated sequence of clicks.

The production Tool/native fixture passed a five-action background program:
click an ordinary non-AX canvas, type text, invoke the real menu with Cmd+Shift+K,
scroll by 8, then type an emoji, followed by one final observation. The covered target received one content
click with unchanged modifiers, text reached the canvas while the separate text
field remained unchanged, and the menu/scroll actions took effect. Exactly one
outside-frame focus pair prepared the entire program. The observer retained
active/key/frontmost and stacking state, received no input, and the human cursor
matched its baseline. Stop deactivated the internally prepared target. Semantic
Unicode field input also passed in the fixture before the program.

The standalone raw native runner has prior passing evidence with strict cursor,
lifecycle and event-scope checks; it was not rerun during this refactor. Both
runners use ordinary inactive controls and preserve the observer baseline;
neither substitutes a permissive first-click implementation. Current validation
counts and platform gaps are recorded in the verification section below.

Complete acceptance must verify no off-frame content hit, exactly one requested
content click, correct text destination, unchanged human foreground/key/stacking
state, modifier fidelity, cursor validity and Stop cleanup through the production
Tool pipeline. Native success on these controls will not establish universal
support for every application, game, Space or protected surface.

Hidden, minimized and unavailable surfaces are distinct from occlusion. A known
locked session yields `SESSION_LOCKED`; unknown session metadata retains normal
capture errors. No unlock, wake or automatic activation is a recovery action.

### Windows

Observation and semantic actions share a retained MTA UIA worker/cache. Validate
PID, HWND, generation, target revision, enabled state and geometry before supported Invoke,
Toggle, SelectionItem or Scroll operations. Release retained COM references with
the cache; do not rebuild indices during input.

Standard Unicode Edit/RichEdit text insertion replaces the current selection
through `EM_REPLACESEL`, preserving text outside that selection. An observed
NodeIdx retains UIA identity. ImageXy resolves through the screenshot map and
recursive `ChildWindowFromPointEx` within the bound HWND, then retains that exact
child's UIA element; it does not hit-test a covering application's global point.
Without explicit focus, `GetGUIThreadInfo` reads the bound window thread's focused
native control. PID, UIA process, root HWND, enabled state and read-only state
must match before input. Unsupported OCR/grid or non-edit targets return
`BACKGROUND_TEXT_UNAVAILABLE`; timeout after submission is an unknown outcome,
not a retry. No clipboard, foreground switch or arbitrary whole-value replacement
substitutes for insertion. The point/focus routes and their native fixture have
cross-compile evidence; the Windows fixture has not been executed.

### Ubuntu

Portal authorization immediately binds the retained stream to the control
resource. A scope change cannot silently replace that binding in the same
generation; stale-generation work is rejected. Portal sessions retain one
authorized PipeWire remote and stream. Prefer stable
PipeWire serial targeting when available. Frame-size changes invalidate absolute
coordinates. Notify input is the single portal seat transport; do not mix in an
unconsented X11/global path. Portal authorization has a bounded deadline and
explicit local-authorization failure; late responses cannot revive old sessions.
Native-thread Stop schedules release/closure through the retained runtime.

AT-SPI caches retain bus/object identity, control generation and target revision. Default actions
execute once without coordinate fallback. Text insertion uses character caret
and selection offsets with UTF-8 byte length. One selected range is replaced;
unselected text remains intact. Invalid offsets or multiple selections fail
explicitly. The bridge uses canonical `NActions`/`GetName` and `GetNSelections`
wire names rather than relying on known incompatible generated/batch calls.

## Persistent visual feedback

The virtual pointer is independent of the human pointer. Its last position lasts
until Stop or target change; only the click ring expires. The arrow uses a rounded
neutral-gray silhouette, soft shadow and a fixed hotspot. Raw model/OCR captures
do not contain the feedback overlay. Input text is not copied into feedback logs.

Native feedback is nonactivating and click-through. macOS hides it while the
target region is covered, retaining its position for reappearance. Windows uses
preview feedback conservatively when native visibility cannot be established.
Ubuntu preview does not invent a Wayland global overlay. Reduced-motion retains
static position feedback without movement transitions.

The physical preview cursor is a deterministic 240 Hz simulation sampled by
`requestAnimationFrame`, with retargeting, directional rotation, speed stretch,
arrival deformation, opacity/blur and resize behavior from the reference. Motion
never acknowledges, schedules or delays native input. Reduced-motion uses
immediate feedback. Rust-generated golden traces test the mathematical port,
not native OS delivery. The click ring expires independently of the persistent
pointer.

The product surface shows authoritative target, mode, status and Stop while the
chat card is collapsed. Preview sampling is bounded and stops when unobserved;
React component lifetime does not own native capture lifetime. Generation/target
checks discard stale preview responses and device-switch results.

## Remote and upgrade compatibility

| Scenario | Required behavior |
|---|---|
| Remote workspace | Existing SSH/Docker workspace refusal; never operate the controller's local GUI as fallback |
| Remote control | Mobile/IM have no App picker or grant UI and receive an explicit unsupported/`SHARE_REQUIRED` state; no hidden blocking prompt; task cancellation remains reachable |
| Peer Device | Share/permission/preview commands proxy to the peer only after `computer_use_sharing_v1` negotiation; controller permission is not peer permission; old peers and CLI without GUI reject explicitly |
| Detached Dispatch | Headless execution has no implied GUI capability or submitter UI dependency; require an explicitly negotiated target provider |

Missing new capabilities on old peers mean unknown/unsupported. Portable fields
use conservative defaults; old payloads remain readable. Do not delete/reset
persisted sessions, settings or profiles to repair incompatibility. Internal Rust
port cleanup does not authorize changing persisted or cross-version wire shapes.
Local fixtures and unsupported-host unit tests are not remote end-to-end evidence.

## Verification and acceptance

The 2026-10-12 refactor started from `f5b6d3ab2`. Focused checks passed: 72 Core
Computer Use tests, 160 Desktop tests, 33 contract tests, five runtime-evidence
tests, eight prompt-catalog tests, 63 isolated production-source context tests,
19 operation-registry tests, nine Desktop remote-workspace policy tests and
82 frontend tests. The frontend command covers sharing/permissions, preview
gestures, stale responses, peer capability gates, Appearance registration and
cursor/geometry parity. The cursor oracle contains 63 samples generated by the
unmodified reference Rust implementation. Counts from separate suites overlap
and must not be added into a unique-test total. Nine Desktop native tests and one
browser context test remain opt-in/ignored in those unit commands; they are not
counted as native passes. The macOS Desktop debug build, `check:web`, i18n audit,
all-surface color audit, repository hygiene, generated capability check and
core-boundary checks also passed.

Dedicated macOS fixtures separately verified retained AX references after cache
cleanup, stale observation-ID rejection before input, the complete Tool/native
roundtrip, strict background input, capture and passive feedback. The final
strict background run used the production App selection, observe-only capture
and explicit grant flow, then exercised native manual-preview input and rejected
its stale request after grant revocation. Observer foreground/key/stacking, its
input-event history and the system cursor assertions all passed, including Stop.
Earlier attempts include a hardware-idle precondition timeout and two failures
of the final unchanged-cursor assertion; they remain failure/inconclusive evidence.
An unchanged rerun during a user-provided quiet interval passed. Neither input
replay, focus restoration nor relaxed assertions were used to obtain that pass.
The separate Unicode/OCR roundtrip initially stopped at its background-start
precondition because macOS selected the disposable target as the foreground app.
Its runner now starts an independent observer after the host initializes; the
rerun passed exact input counts, Unicode text, stale-reference rejection, OCR,
generation/binding continuity and foreground preservation.

The Windows production native modules pass cross-compilation with
`test-windows-computer-use-native.mjs --check --target x86_64-pc-windows-msvc`.
A full Desktop cross-build on macOS is blocked by the unavailable Windows C SDK
headers required by `ring`; module type-checking does not cover that build or
native execution. Linux production modules were compiled and executed in an
ARM64 Debian container with real D-Bus, AT-SPI, GTK and Xvfb: 65 focused tests
passed, with two opt-in native tests ignored; the separate GTK fixture then
passed button activation and UTF-8 EditableText insertion while preserving
existing text. This validates the production AT-SPI provider under Xvfb, not a
full Linux Desktop build or compositor integration. No GNOME Wayland compositor,
portal chooser or live PipeWire stream was available for the portal fixture.

The sharing card was rendered in Chromium with the real design-system styles at
1000×800 and 390×700; the narrow layout keeps Stop visible while permissions are
expanded. That visual harness mocks transport. It is not an end-to-end Tauri
permission-dialog test. Full OS denied-to-granted/restart flows and system Stop
Sharing UI interaction remain manual acceptance work.

Run focused checks from the [Desktop Computer Use guide](../../src/apps/desktop/src/computer_use/AGENTS.md)
and [Core guide](../../src/crates/assembly/core/AGENTS.md). A build-only result,
ignored native test or permissive first-click fixture is not a successful strict
background-input run.

| Area | Recorded evidence | Remaining scope |
|---|---|---|
| macOS strict production Tool pipeline | User-selected share and separate grant; five-action non-AX canvas/text/menu/scroll/emoji program with one final observation; exactly one content click and one outside-frame focus pair; native preview input and revocation; observer foreground/key/stacking and human cursor unchanged; no observer input; Stop deactivated target | Wider applications, cross-Space behavior and visual system Sharing UI acceptance |
| macOS capture/feedback | Dedicated native fixtures exercised obscured target pixels, binding retention, movement/resize, hidden/minimized errors, closure and pointer lifetime; earlier locked-session rejection evidence was not rerun | System Stop Sharing interaction and broader platform/application matrix |
| Linux text semantics and scope | 65 production-module tests plus a real GTK/AT-SPI activation and UTF-8 insertion fixture under D-Bus/Xvfb; portable admission, permission, grant and geometry checks | Full Linux Desktop build, GNOME Wayland portal consent, real PipeWire/seat input and sharing UI; arbitrary background coordinates remain explicitly unsupported |
| Windows native modules | Production-source cross-compilation of control admission, UIA/MSAA, WGC, pointer feedback, permissions, process identity, grants and background witnesses | Full Windows Desktop build, native delivery, WGC border/stop and foreground preservation |
| Permissions and remembered grants | Read-only probe/explicit request tests, process identity checks, private atomic persistence, legacy/future-field retention, grant revocation and no automatic sharing | Real OS permission denial/approval/restart matrix and broader executable upgrade scenarios |
| Preview and virtual cursor | Rust golden parity, padded/negative-origin/resize geometry, one-input dispatch, pointer ownership, cancellation and two responsive visual renders; native preview Escape delivered once | Full desktop UI-to-native pointer/drag matrix across all three OSes |
| Remote scenarios | Explicit remote-workspace/headless-provider refusal tests; frontend peer-scope, old-peer capability denial and stale-preview tests | No remote-workspace, Remote Connect, Peer Device or Detached Dispatch end-to-end GUI validation |

The macOS strict fixture builds before opening windows, initializes the host,
uses separate application bundle identities, and waits for a bounded hardware
input idle interval before foreground setup. Busy desktop timeout is
`INCONCLUSIVE`, not PASS. After the baseline, it never restores the observer's
focus or stacking order. Activation history detects transient focus changes;
assertions retain failure evidence. Fixtures manipulate only their disposable
windows and send no real messages. Timings are fixture observations, not promised
whole-task speedups or performance across platforms.

Release acceptance additionally requires:

1. Obscured capture returns the selected surface with no covering pixels or
   unsolicited full-desktop fallback.
2. System/product Stop, cancellation, permission loss and target closure reject
   queued/stale-generation work and release held inputs without resurrection.
3. Background native input preserves human foreground, focus, pointer and stacking
   state while another fixture is used; unsupported paths report limitations.
4. Receivers observe exactly one submitted action; failed final observation never
   repeats input. Partial programs expose completed and unattempted steps.
5. Coordinate tests cover stale IDs, rebinding, resize, Retina/mixed DPI, negative
   monitor origins, window movement and same-PID foreign windows.
6. Model messages carry the actual image and its matching identity/geometry;
   text-only paths supply real AX/OCR evidence rather than empty success.
7. Portal denial, missing devices/streams/plugins, closed sessions and revoked
   resources fail explicitly and clean up without local fallback.
8. Legacy deserialization and unsupported-host defaults pass; native OS and remote
   scenarios are reported separately rather than inferred from shared tests.
