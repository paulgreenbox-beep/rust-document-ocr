# Publisher confirmation — Microsoft CRT

**UNSIGNED TEMPLATE — NO REDISTRIBUTION APPROVAL.** Complete and retain the
signed evidence privately. Do not commit contract identifiers, entitlement
records, account details or signatures to the public repository.

Applies only to the three unchanged x64 files and manifest SHA-256 in
[REDIST evidence](REDIST_EVIDENCE.md). Approval of this scope does not
approve different DLL bytes, a separate redistributable download, other
products or the Windows release as a whole. The app-local file match does
not settle the terms for compiler/runtime shims retained statically or the
license scope of MSVC/Windows SDK library inputs. Keep that review separate
and distinguish imported DLL symbols from statically retained code.

| Required answer | Publisher/reviewer to supply |
| --- | --- |
| Publisher | Publishing legal entity or person; authorized signatory and role; applicable license holder and relationship to publisher |
| Acquisition and eligibility | Product/edition, applicable Visual Studio/Build Tools license and channel (retail, subscription, volume or other); evidence that actual users, organization, build activity and publisher are covered |
| Affirmative grant | Agreement title, revision/effective date, applicable product/channel, exact Distributable Code provision or other written Microsoft grant; any incorporation of the 2026 REDIST list and why the grant reaches this publisher |
| Scope | Program/version, approved manifest and three DLL hashes, app-local copying with that program, intended recipients and public distribution channel, territories and any downstream restrictions |
| Conditions | Reviewer-approved recipient terms/notices required by the actual grant; unchanged files, no debug/prerelease payload, no standalone distribution outside the grant |
| Servicing | Named owner and cadence for tracking Microsoft advisories, checking exact replacement bytes/rights, updating app-local files and requalifying the package |
| Decision | **APPROVED FOR EXACT SCOPE / HOLD / NOT APPLICABLE WITH WRITTEN BASIS**; evidence reference, reviewer, authorized signature and date |

Suggested sign-off, only after verification of the blanks:

> Acting for [publisher], I confirm that [applicable agreement and exact
> provision or written permission], held by [eligible license holder],
> authorizes [publisher] to distribute the three unmodified REDIST DLLs
> identified by [manifest/file hashes], app-locally with [program and
> version], through [channel] to [recipients/territories], subject to
> [conditions]. Evidence is retained as [private reference]. The publisher
> will meet [recipient terms/notices and servicing responsibilities]. This
> confirmation excludes any other package or component.

If the grant or app-local distribution scope cannot be documented, record
**HOLD**. A valid signature, REDIST path, upstream list or this blank form
does not waive any copyright, patent or clean-Windows gate.
