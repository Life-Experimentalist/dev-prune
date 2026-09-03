# Security Policy

## Supported Versions

Fixes ship in a new release rather than being backported, so the supported version is
the latest one.

| Version        | Supported                        |
| -------------- | -------------------------------- |
| 1.19.x         | ✓                                |
| Anything older | ✗ — upgrade with `devp update`   |

## Reporting a Vulnerability

We take the security of `dev-prune` very seriously. As a tool that performs filesystem operations, safety and data protection are our highest priorities.

If you discover a security vulnerability or a potential risk of accidental data deletion:

1. **Do NOT open a public GitHub issue.**
2. Email your findings directly to security.devprune@vkrishna04.me or contact the maintainer directly.
3. Provide details including:
   - Operating System and version
   - Steps to reproduce the issue
   - Expected vs actual behavior
   - Code snippet or sample repository setup

We will acknowledge receipt within 24 hours and issue a patch as quickly as possible.

## Verifying a release

Every archive on a release page ships a `.sha256` sidecar, and every archive is signed
by GitHub's build provenance — a Sigstore attestation binding the file to this
repository, the workflow that built it and the commit it was built from. The checksum
proves the file was not altered in transit; the attestation proves where it came from,
which the checksum cannot, because the same hand that swaps an archive swaps its
sidecar.

```powershell
gh attestation verify dev-prune-v1.19.0-windows-x64.zip --repo Life-Experimentalist/dev-prune
```

```bash
gh attestation verify dev-prune-v1.19.0-linux-x64.tar.gz --repo Life-Experimentalist/dev-prune
```

The npm package carries npm provenance (`--provenance`) and the PyPI package is
published through Trusted Publishing, so both can be traced back to the same workflow
run without taking anyone's word for it.

## Anti-virus detections

Endpoint products sometimes flag `devp.exe` with a generic, machine-learning name —
`Mal/Generic-S`, `Trojan.Generic`, `Heur.AdvML.B`. A generic name means a classifier
scored the file, not that anything matched a known sample.

Some of that was earned, and is fixed. Through 1.14.x the binary drove `powershell.exe`
with an encoded command to edit `PATH`, generated the windowless `devpw.exe` by writing
a modified copy of its own image to disk, and left a detached shell behind to delete
itself during `uninstall`. Those are three of the shapes a heuristic is built to catch,
and none of them was necessary. As of 1.15.0 the registry is written directly, `devpw`
is an ordinary build target that ships in the archive, and **no code path starts a
process that outlives the command that started it.**

What remains is the tool itself: it deletes directories, makes network calls and hashes
downloads, which together are also the ransomware template. No rewrite changes that, and
we would rather say so than pretend the shape is gone.

If your scanner flags a release, the useful response is to
[report it as a false positive](docs/troubleshooting/INSTALLATION_ISSUES.md#13-my-anti-virus-quarantined-devpexe) —
that is what retrains the classifier — and to open an issue here with the vendor,
detection name and version. Please do not add a folder exclusion for
`%APPDATA%\dev-prune\bin\`: it silences that directory for every future detection,
including real ones.
