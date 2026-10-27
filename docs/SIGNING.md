# Windows code signing (SignPath Foundation)

The release workflow signs `PicoBar.exe` and the installer when the repo
variable `SIGNPATH_ORGANIZATION_ID` is set. Otherwise it builds unsigned,
exactly as before.

## Flow (two signing requests)

1. Build `picobar.exe`, upload as artifact `unsigned-exe`, sign, overwrite
   `target/release/picobar.exe` with the signed file.
2. The existing step builds the portable zip and the Inno Setup installer
   from that signed exe.
3. Copy the installer to `stage/PicoBar-setup.exe` (fixed name, so the
   artifact configuration needs no wildcard), upload as `unsigned-setup`,
   sign, copy the signed file back to the versioned name. Each signed file
   must report Authenticode status `Valid` or the job fails.
4. The `release` job downloads only the `dist-*` artifacts (never the
   unsigned ones) and runs `sha256sum` over them, so
   `SHA256SUMS.txt` covers the signed files. The updater's checksum check
   keeps working.

Unsigned artifacts are kept for 1 day only. The Windows job waits up to
1 hour per request (job limit 180 min).

**Approvals must happen while the run is live**: the job waits for each
request, and times out if nobody approves in time. Re-running the failed
job submits new signing requests (the old ones are not reused), so approve
the new ones.

Two requests are needed because the installer does not exist until the exe
is signed. The installed uninstaller stub is not signed (Inno limitation).

## One-time setup

1. Check the requirements at <https://signpath.org/terms> (OSI license: MIT
   ok; README "Code signing policy" section: done) and apply at
   <https://signpath.org/apply>. Wait for approval.
2. In SignPath, add the predefined **GitHub.com** Trusted Build System to
   the organization and link it to the project.
3. Create project with slug `picobar` (repo URL
   `https://github.com/lido772/picobar`). Enable MFA.
4. Create signing policy slug `release-signing`: certificate = the
   Foundation certificate, manual approval required, origin restricted to
   your repo and tags. Optionally pipeline policy `runners:
   require_github_hosted: true`.
5. Create two artifact configurations (the GitHub artifact is a ZIP, so the
   root is `<zip-file>`). Slug `exe` (set as default):

   ```xml
   <artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
     <zip-file>
       <pe-file path="picobar.exe">
         <authenticode-sign/>
       </pe-file>
     </zip-file>
   </artifact-configuration>
   ```

   Slug `installer`:

   ```xml
   <artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
     <zip-file>
       <pe-file path="PicoBar-setup.exe">
         <authenticode-sign/>
       </pe-file>
     </zip-file>
   </artifact-configuration>
   ```

   Product name/version metadata must be consistent (Foundation rule); add
   `<pe-file-set>` metadata restrictions if you want them enforced.
6. Create an API token for a submitter user. In GitHub (Settings, Secrets
   and variables, Actions):
   - secret `SIGNPATH_API_TOKEN`
   - variable `SIGNPATH_ORGANIZATION_ID` (setting this turns signing on)
7. Optional: install the SignPath GitHub App (audit-log evaluation).
8. Tag a release, approve the two requests in SignPath. If a slug differs,
   edit `project-slug` / `signing-policy-slug` in
   `.github/workflows/release.yml`.

## Protect release tags

Add a GitHub tag ruleset for `v*` (Settings, Rules, Rulesets): restrict
creation, updates and deletion to maintainers, so only trusted people can
trigger a signing run (SignPath's origin check relies on this).

## Verify

```powershell
Get-AuthenticodeSignature .\PicoBar.exe | Format-List Status, SignerCertificate
signtool verify /pa /v .\PicoBar.exe
```

Status must be `Valid`. Check both the exe inside the zip and the installer.
