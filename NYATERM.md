# NyaTerm fork notes

This branch carries [NyaTerm](https://github.com/nyakang/nyaterm)'s local changes
to `russh-sftp` on top of an unmodified upstream base.

- Fork: <https://github.com/nyakang/russh-sftp>
- Upstream: <https://github.com/AspectUnk/russh-sftp>
- Base revision: `2ef937985fa965a3e1a8a83828ce7acab30719d6` (upstream `master`, 2026-10-13)
- Branch: `nyaterm`

These changes prevent stalled writes and leaked server handles during uploads,
downloads, remote editing, cancellation and error cleanup, and let a caller
address remote paths that are not valid UTF-8.

## Patches

1. `feat: carry SFTP path fields as raw bytes` — protocol path fields become
   `Vec<u8>` via `serde_bytes`, with `_bytes` counterparts on the client APIs.
   The `String` APIs are unchanged and delegate through UTF-8. Server-side path
   packets are bridged back to the `Handler` string interface with lossy
   conversion. Handle bytes, file contents, and OpenSSH extension payload
   schemas are untouched.
2. `fix: make request and handle lifetimes self-cleaning` — `PendingRequest`
   owns its timeout and removes its own pending-map entry on timeout and on
   drop; a stream failure wakes every pending request through a `fail_pending`
   callback instead of leaving each to time out; a late reply to a timed-out
   request is logged instead of terminating the handler loop; `close` releases
   the handle on every outcome; dropped files close through the tracked
   `close_detached` instead of the untracked `close_nowait`; shutdown is
   idempotent and a stuck write acknowledgement surfaces as `TimedOut`.
3. `feat: expose positional reads, OpenSSH symlink order, and server limits` —
   `File::read_at`, `symlink_openssh` (OpenSSH swaps the symlink argument order
   relative to the draft this crate follows), and `SftpSession::limits` /
   `max_open_handles` / `effective_max_packet_len`.
4. `test: cover request, handle, and limit lifetimes`.

## Not carried here

- The NyaTerm snapshot repoints the `russh` dev-dependency to a sibling
  `vendor/russh` path and keeps a committed `Cargo.lock` (with a `!/Cargo.lock`
  ignore override). Both are vendor-layout concerns: this branch keeps
  upstream's `russh = "0.62.5"` dev-dependency and no lock file. A consumer that
  needs the patched russh should express that with a `[patch]` entry in its own
  workspace.

## Validation

```sh
cargo test --lib          # 16 passed
cargo check --all-targets
```

The 2026-09-22 merge to `c2776c64c2` conflicted in the client request,
file-I/O, runtime, and high-level session layers. The resolution adopts
upstream 3.0's `Request` future, creation-time deadlines, borrowed write
encoding, and pipelined reads/writes. It retains raw-byte path APIs, OpenSSH
symlink ordering, server-limit accessors, POSIX rename, explicit transport
failure wakeups, tracked detached close, release-on-all-outcomes handle
accounting, idempotent shutdown, and `TimedOut` I/O classification.


## Atomic replacement API (UI/UX migration)

Add `SftpSession::posix_rename_bytes`, gated on the advertised OpenSSH extension.
This lets NyaTerm replace a symlink without unlinking the existing path first.
Unsupported servers fail without mutation. Payload tests cover byte paths/order;
an unsupported-session regression checks failure without a protocol request.
Validation: Windows, `cargo test --lib`, `cargo fmt --all -- --check`.

## 2026-10-01 upstream merge

Merged upstream 3.0.1. Conflicts in the client runner, raw session and file
shutdown retain raw-byte paths, POSIX rename, request failure wakeups and tracked
detached close. Incoming packet limits are threaded through the configured
runner; handle accounting releases on every CLOSE outcome. Shutdown marks the
file closed when CLOSE is queued, preventing a duplicate CLOSE during drop.
Windows validation: `cargo test --lib` (18 passed), `cargo check --all-targets`,
`cargo clippy --all-targets -- -D warnings`, and `cargo fmt --all -- --check`
passed. New regressions cover oversized lengths waking requests without waiting
for payloads and cancellation while a CLOSE acknowledgement is pending.

## 2026-10-13 upstream rebase

Upstream `master` is fixed at `2ef937985fa965a3e1a8a83828ce7acab30719d6`.
Previous published NyaTerm head: `aba3096b600dbbecbe7172dd4dec946667346c6b`.
The previous branch is retained locally as `backup/nyaterm-20261013`.

The effective old-base-to-fork tree (including merge-only resolutions) was
reconstructed byte-for-byte as separate concern commits before rebasing.
Old upstream merge history was flattened; no functional net change was discarded
except the explicitly documented upstream replacements below. The concern
commits and subsequent compatibility fixes remain separate.

| Concern | Reconstructed commit on old base |
| --- | --- |
| raw byte paths | `a3dc5f658212b8aef8a2a6dec7acd46ed4184506` |
| request and handle lifetimes | `497dd90dbf8b5cada9e2dbfbc917826889a137af` |
| positional reads and server limits | `e863629fe7fd71267e52094bffef5bbb69c3f5a8` |
| atomic byte-path POSIX rename | `00139316f62fdc3d0300509dd037efbdcd6d1384` |
| fork validation | `930777ed61b6680b34bbae098b5bdda2d0df4a8c` |
| fork documentation | `3ebc67f83c9b039a2375cef04e89758bfba1f731` |

Replay commits before this documentation update:

- `ad57f50c27ace56b99bec2988cebb7f83b53abb9` feat(fork): preserve raw byte paths
- `a7d4278987fe0d0ec7749cddc4fa477ea031b418` feat(fork): preserve request and handle lifetimes
- `86a4482c48c7db8d6a3d2b53a2496772ca7583b5` feat(fork): preserve positional reads and server limits
- `b8b61950d1342d9f11779942f1cd1401cd5d335f` feat(fork): preserve atomic byte-path POSIX rename
- `d3f2b3aaf209b948acdfc96c6717cd7fe877e155` ci(fork): preserve fork validation
- `b54a2c0091eb4ce471356b904a181c7432967093` docs(fork): preserve fork documentation
- `46b9d73ff9e41a44f0b54707f1ee231e6e981417` fix(client): preserve handle release across Rust versions

Preserved byte-path request methods, non-UTF8 path handling, POSIX byte rename,
pending-request cancellation, handle cleanup, positioned reads and server limits.
The Rust 1.99 atomic API deprecation is handled with a checked compare-exchange
loop that retains older compiler support and the reference-count underflow warning.

Windows: 18 library tests passed; all-target check, strict all-target clippy and
`cargo fmt --all -- --check` passed. Request cleanup and handle-release regressions
run in the library suite.


Validation is on Windows; native macOS/Linux checks are delegated to the
branch workflow. Interactive GUI/file-drop and real-server authentication
checks were not performed by this update. Their outcomes must not be inferred
from scripted peers or compile checks.
