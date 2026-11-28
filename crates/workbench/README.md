# Resume Workbench

Run the app from the workspace root:

```sh
cargo run -p workbench --bin resume-workbench -- /path/to/job-folder
```

Without a folder argument, choose **Open Folder**.
The shell shows resumes, letters, specs, profiles and styles, including built-in styles.
Document tabs are read-only.
Building PDFs, tailoring controls and editing belong to later workbench tasks.

## Navigation

| Action | macOS | Windows |
|---|---|---|
| Open folder | Command+O | Ctrl+O |
| Quick open | Command+P | Ctrl+P |
| Command palette | Command+Shift+P | Ctrl+Shift+P |
| Refresh files | Command+R | Ctrl+R |
| Next problem | F8 | F8 |
| Dismiss palette | Escape | Escape |

Select a problem to open its source location.
Markdown diagnostics retain their codes; spec and Typst syntax errors show their messages without invented codes.
Spec validation includes profile resolution, not a PDF build or baseline selection validation.

## Disk changes and layouts

Clean documents reload after disk changes.
Retained unsaved text is not overwritten: the conflict banner offers Reload or Keep mine.
If a file disappears, its retained text remains available.
Explicitly allow discarding retained text before closing it or changing folders when recovery is no longer wanted.

Dock layouts are saved under the OS configuration directory in `resume-workbench/folders/`, not in the job folder.
The scanner skips hidden entries, `out`, `target`, and `node_modules`, and does not follow symbolic links.
Scans stop at 32 directory levels, 10,000 visited entries or 32 MiB of loaded text, with a visible problem when a limit is reached.
Individual documents are limited to 1 MiB, and specs/profiles keep the engine's smaller size limit.

## Validation status

The macOS shell is implemented under `rsm-yjj.6`.
Windows native validation is deferred by the owner to `rsm-yjj.26`; it has not been counted as passed.
The `spike` binary remains available for the separate `rsm-yjj.1` verification.
