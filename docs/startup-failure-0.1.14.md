# v0.1.14 Windows startup crash

## Confirmed cause

The distributed x64 executable contains an unconditional AVX512-FP16 `vmovw`
instruction in the first tray menu's initialization. On the investigated Windows
11 Pro build 26200 / AMD Ryzen 9 7950X3D system, executing it raises
`STATUS_ILLEGAL_INSTRUCTION` (`0xC000001D`, process exit code `-1073741795`).
The process exits before tray creation completes.

The release workflow built all Rust crates with `-C target-cpu=native`. That
permits instructions supported by the build machine, regardless of the machines
that will run the downloaded asset. CI previously built without that override
and did not launch the GUI executable. Library tests cannot exercise the
binary-only GUI/tray initialization.

The fix explicitly uses `-C target-cpu=x86-64` in both the release and Windows CI
jobs, and launches the resulting executable before upload. It preserves the
existing optimization profile and dependencies. No application behavior, logging,
configuration migration, or single-instance handling needs to change.

## Distributed-binary evidence

Investigation date: 2026-09-05. Both hashes match GitHub's release asset metadata.

| Artifact | Size (bytes) | SHA-256 |
| --- | ---: | --- |
| Installed v0.1.13 | 19,729,408 | `49acb9d3eecff8c69e0dbb2c56d0265a1d7b38e367f685ebecb59d055b6c3a59` |
| Downloaded v0.1.14 | 24,759,808 | `d04aa11b02769d51fc44ac1447d8e0960996fa970bc9418bd3d04e888c5f5fbc` |

The unmodified v0.1.14 asset was launched after the installed instance had exited.
Windows Application Error event 1000 reports exception `0xc000001d` in the EXE at
RVA `0x50a14e`. CDB independently stops on the first-chance exception at the same
instruction; its module inventory shows the CRT, graphics and Windows dependencies
loading successfully. This is an execution fault, not an unresolved DLL import.

```text
Function: RVA 0x50a0d0
Counter:  RVA 0x16e1580, initial value 1000
RVA 0x50a14e: 62 b5 7d 08 6e 04 50    vmovw xmm0, word ptr [rax+r10*2]
At fault: EBP = R9D = 0x3e8 (1000), R10D = 10, R11D = 0
Later in the same function:
RVA 0x50a23f: call [CreateMenu]
RVA 0x50a248: call [CreatePopupMenu]
```

The counter, integer-to-decimal digit table and subsequent Win32 menu calls match
`muda::platform_impl::windows::Menu::new`: it obtains a counter starting at 1000,
converts it to a string for `MenuId`, then calls `CreateMenu`/`CreatePopupMenu`.
EasyHDR calls this through `Menu::new()` in `src/gui/tray.rs`. The captured stack's
EXE RVAs are `50a14e -> 6ef4c -> 56bb4 -> 285a6 -> 12272b -> 1013e5f`.
The asset is stripped, so nearest-export labels in a debugger are not reliable
Rust function names; the mapping above uses instructions, data and source.

Host CPUID leaf 7, subleaf 0 returned EDX `0x10`: bit 23 (AVX512-FP16) is clear.
Intel's specification lists this instruction as requiring AVX512-FP16. The
instruction therefore cannot execute on this host, even though other SIMD
extensions are available.

The existing environment sets `RUST_LOG=warn`, suppressing normal startup progress.
Repeating under a child-process-only `RUST_LOG=info` ends the log at "Creating
system tray icon", consistent with the debugger. The CPU exception bypasses
normal Rust error logging; adding log messages would not fix it.

## Why v0.1.13 works

Both release tags contain `target-cpu=native`; this was an existing portability
defect. The v0.1.14 release run confirms Rust 1.98.0 and runner image
`windows-2025-vs2026`, version `20260818.207.1`, and shows the native CPU flag on
the actual compiler invocations. Between the tags, Slint and tray-icon/muda and
other locked dependencies changed, while application startup changes were
comments and a deprecation-lint allowance. Release packaging only changed the
checkout action reference; it still uploaded `target/release/easyhdr.exe`.

Decoding PE unwind-described functions found 225 AVX512-FP16 instructions in
v0.1.14 and none in v0.1.13 or the corrected local build. This is supporting
evidence, not an exhaustive ISA certification: it does not scan all leaf
functions or distinguish runtime-dispatched code from unconditional code.
The faulting menu path itself is unconditional and directly observed.

The v0.1.13 Actions logs have expired (HTTP 410). The exact older runner CPU and
compiler cannot be established from those logs. A particular runner CPU change
or compiler optimization change is therefore not asserted as the sole trigger.
The confirmed defect is allowing the hosted machine's ISA into a public binary;
the two assets demonstrably differ in the relevant emitted instructions.

## Regression and reproduction

Run on a Windows desktop with no other EasyHDR instance:

```powershell
$env:RUSTFLAGS = '-C target-cpu=x86-64'
cargo build --release --locked
./scripts/Test-ReleaseStartup.ps1 -Executable target/release/easyhdr.exe
# Also test a real previous-release exit followed by upgrade and restart:
./scripts/Test-ReleaseStartup.ps1 -Executable target/release/easyhdr.exe `
    -PreviousExecutable 'D:\Apps\EasyHDR\easyhdr.exe'
```

The smoke test uses a new child-only APPDATA directory, no monitored applications,
and a visible window. It requires tray initialization, an actual titled window,
and Slint event-loop readiness, observes the process for three more seconds,
sends WM_CLOSE to that window, requires exit code zero, and restarts with the
same saved configuration. It retains logs under the temporary directory printed
at the start. Failure cleanup terminates only the process the test started.

The distributed v0.1.14 fails this test on the affected CPU with `0xC000001D`;
v0.1.13 and the corrected build pass. A smoke test on a CPU supporting FP16 alone
cannot detect this portability defect. The explicit baseline in the build jobs
is essential; runtime smoke coverage complements it and catches loader and GUI
startup failures before assets are uploaded.

## Local validation results

The corrected optimized build uses Rust 1.98.1, MSVC 14.44.35207 and Windows SDK
10.0.26100.0 on the affected host. The application Rust source and lockfile are
unchanged by this fix.

| Check | Result |
| --- | --- |
| Distributed v0.1.14 smoke test | Fails as expected, `0xC000001D` before readiness |
| Installed v0.1.13 -> corrected build -> corrected restart | All three visible-window launches and normal exits pass |
| Real existing APPDATA, minimized-to-tray preference | v0.1.13 exit, corrected upgrade and corrected restart pass; Exit dispatched through the tray menu's Win32 command handler |
| Formatting / all-targets, all-features Clippy with warnings denied | Pass |
| `integration_tests` | 10 passed |
| `version_detection_tests` | 12 passed |
| `memory_usage_test` | 19 passed |
| `startup_time_test` | 12 passed |
| Library tests, normal parallel mode | 197 passed, 4 failed |
| Library tests, single-threaded | 199 passed, 2 failed |
| `cpu_usage_test`, repeated without concurrent local builds | 2 passed, 1 failed |

The wider suite is **not fully green on this hardware**. Parallel library failures
were `test_run_processes_events` (100 ms receive deadline after a 50 ms sleep),
`test_hdr_state_toggle_timing` (733 ms versus a 500 ms limit), and the two HDR
restoration tests. Sequential execution passed the restoration/toggle tests, but
retained the event timeout and failed `test_disabled_uwp_app_ignored`, which assumes
HDR is initially off. That last test passes individually after restoring HDR off.
The CPU interval test exceeded its 2% threshold (2.19% initially; 2.50% on repeat).
These tests and application code are unchanged; their hardware/state/timing
limitations were not suppressed or adjusted as part of the startup fix.

The original executable and user preference values were preserved. Normal app
launches rotated logs and refreshed the update-check timestamp. HDR was restored
to its initial off state after the hardware tests, and test instances were exited.
Crash dumps, module lists, disassembly, CPU results and detailed logs are retained
in the local investigation directory rather than committed (dumps and config
copies may contain private machine data). Cleanup is documented there separately.

## References

- [v0.1.14 release build log](https://github.com/engels74/EasyHDR/actions/runs/32577998488)
- [Rust target-cpu semantics](https://doc.rust-lang.org/rustc/codegen-options/index.html#target-cpu)
- [Intel AVX512-FP16 architecture specification](https://cdrdv2-public.intel.com/678970/intel-avx512-fp16.pdf)
