# SABnzbd API compatibility (what Sonarr and Radarr need)

Swarmy answers SABnzbd's API so Sonarr and Radarr can use it as a
"SABnzbd" download client. This is the contract, read from their source:
Sonarr `develop` @ c018578e, Radarr `develop` @ 2f64b5cf
(`src/NzbDrone.Core/Download/Clients/Sabnzbd/`), against SABnzbd 5.1.3.

## Requests

- URL: `{http|https}://host:port{/urlBase}/api`, no trailing slash, redirects
  not followed. Empty URL base means `/api`; Swarmy also serves `/sabnzbd/api`.
- GET for everything except `addfile` (POST, multipart).
- Query order: `mode`, call parameters, then `apikey=` (or
  `ma_username=`/`ma_password=`), then `output=json`. Accept both auth forms.
- `mode=version` needs no key.
- Any HTTP status >= 400 is a generic "unable to connect". Errors should be
  HTTP 200 with `{"status": false, "error": "..."}`. A bad key should say
  `API Key Incorrect` (or `API Key Required`) so Sonarr shows its proper message.
- JSON: camelCase-insensitive, unknown fields ignored, nulls ignored.
  Send what SAB sends: `mb`/`mbleft`/`percentage` as numeric strings, `bytes`
  as an integer, misc flags as 0/1, booleans as JSON booleans.
  Content-Type `application/json; charset=utf-8`.

## Calls

| Call | Request | Response |
|---|---|---|
| Version | `mode=version` | `{"version":"4.5.5"}` (regex `\d+\.\d+\.(\d+|x)`; >= 1.0 passes) |
| Config | `mode=get_config` (no `section`) | `{"config":{"misc":{"complete_dir":"/abs/path","pre_check":0,"history_retention_option":"all","history_retention_number":1},"categories":[{"name":"*","dir":"","priority":0,"pp":"3","script":"None"},{"name":"tv","dir":"tv",...}],"servers":[],"sorters":[]}}` |
| Full status | `mode=fullstatus&skip_dashboard=1` (only if `complete_dir` isn't absolute) | `{"status":{"completedir":"/abs"}}` |
| Queue | `mode=queue&start=0&limit=0[&category=X]` | `{"queue":{"paused":false,"slots":[{"index":0,"nzo_id":"…","filename":"Title","cat":"tv","priority":"Normal","status":"Downloading","mb":"1024.00","mbleft":"512.00","percentage":"50","timeleft":"0:04:10"}]}}` |
| History | `mode=history&start=0&limit=60[&category=X]` | `{"history":{"slots":[{"nzo_id":"…","name":"Title","nzb_name":"Title.nzb","category":"tv","bytes":1073741824,"status":"Completed","storage":"/abs/complete/tv/Title","fail_message":"","download_time":300}]}}` |
| Add | `POST mode=addfile&cat=X&priority=N`, one multipart part `name="name"; filename="Title.nzb"` | `{"status":true,"nzo_ids":["SABnzbd_nzo_…"]}` |
| Remove (queue) | `mode=queue&name=delete&del_files=0|1&value=ID` | `{"status":true,"nzo_ids":["ID"]}` |
| Remove (history) | `mode=history&name=delete&del_files=0|1&value=ID&archive=0|1` | `{"status":true}` |

Key details:

- `queue.slots` and `history.slots` must be arrays, never null. Queue uses
  `cat`, history uses `category`. `filename` and `timeleft` must be present
  and non-null; `timeleft` is `H:M:S` or `D:H:M:S` only.
- Status strings (exact): Grabbing, Queued, Paused, Checking, Downloading,
  QuickCheck, Verifying, Repairing, Fetching, Extracting, Moving, Running,
  Completed, Failed, Deleted, Propagating. An unknown string breaks the whole
  queue parse. Queued/Grabbing/Propagating mean queued; Paused (or global
  pause) means paused; everything else is downloading. In history only
  Completed and Failed are terminal.
- Priority: -100 Default, -2 Paused, -1 Low, 0 Normal, 1 High, 2 Force.
  Queue reports it as a name ("Normal", "Force"…).
- `nzo_id` must be identical across add, queue and history.
- An empty `nzo_ids` with `status:true` means "rejected, possible duplicate"
  (no fallback to another client); `status:false` makes Sonarr fall back.
- Titles starting `ENCRYPTED / ` are treated as failed (encrypted).
- Output path: `storage`, walking up to the ancestor named exactly `name`.
  Use `<complete_dir>/<category dir>/<name>` with `name` the folder name.
- Categories: the client's category (default `tv` / `movies`) must exist in
  `get_config.categories` with a non-null `dir` not ending in `*`, else Test
  fails. Send `"sorters": []` and omit the legacy sorting flags.
- `history_retention_option: "all"` avoids the "removes completed downloads"
  health warning.
- Test button: `version`, then `get_config` three times.
- After import (with "Remove completed"), Sonarr deletes the files itself and
  sends `mode=history&name=delete&del_files=1&value=ID&archive=1`.
