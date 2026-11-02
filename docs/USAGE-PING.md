# Usage ping

Every Anilili app (Android phone and TV, desktop, iOS) sends the same once-a-day ping so the
project can count its daily and monthly users, follow retention and reinstalls, and see platforms,
versions, places, devices, settings, how much the app is used, and which devices belong to one
person. This file is the contract; the same copy is `docs/USAGE-PING.md` in each app. Change them
together.

**Version 2** (from 2026-10-13) adds `people`, `env`, `settings` and `usage`. The server takes
both versions: every version 2 field is optional, and one that is missing or malformed is
dropped rather than refused, so an app can add them one at a time.

## What is sent

```
POST https://pulse.anilili.workers.dev/v1/ping
Content-Type: application/json

{"device":"3f1c…(64 hex)","platform":"android-tv","version":"0.1.90","build":101,"accounts":["anilist"],"first":false,
 "people":["9a0e…(64 hex)"],
 "env":{"os":"11","api":30,"maker":"Amazon","model":"AFTKA","screen":"1920x1080","lang":"sq-XK","app_lang":"sq"},
 "settings":{"quality":"1080","theme":"dark","audio":"sub","player":"internal"},
 "usage":{"minutes":95,"watched":60,"sessions":3}}
```

At most 4096 bytes. The server answers `204`.

| Field | Value |
| --- | --- |
| `device` | The device ID: 64 lowercase hex characters (below). |
| `platform` | One of `android`, `android-tablet`, `android-tv`, `iphone`, `ipad`, `windows`, `macos`, `linux`. Anything else is refused (400). |
| `version` | The app's version: `major.minor.patch` (`0.1.83`), or `major.minor`. |
| `build` | The build number (Android's `versionCode`, iOS's `CFBundleVersion`); `0` when there is none (desktop). |
| `accounts` | The list services signed in right now, any of `anilist`, `mal`, `simkl` (in any order); `[]` when none. Anything else is refused. |
| `first` | True when this app's data has no record of an accepted ping: a fresh install, or one whose data was cleared. |
| `people` | Version 2. One hash per signed-in list account, in the order AniList, MAL, Simkl (below). `[]` or absent when none. |
| `env` | Version 2. The device (below). |
| `settings` | Version 2. The app's main settings (below). |
| `usage` | Version 2. What happened since the last accepted ping (below). |

The server adds, from Cloudflare's lookup of the connection, the **country**, **city**, region
and the city's approximate coordinates, and a **network tag**: a hash of the day and the
connection's network (an IPv4 address, or an IPv6 address's /64) under a secret key, which only
matches another device's tag on the same network the same day. The IP address itself is never
stored; it also keys an in-memory rate limit.

### The device ID

`device` = lowercase hex SHA-256 of `"anilili-usage-v1:" + <raw ID>`, where the raw ID is one that
survives uninstalling the app, so a reinstall is recognised as the same device:

| Platform | Raw ID |
| --- | --- |
| Android | `Settings.Secure.ANDROID_ID` (per device and signing key; a factory reset changes it). If it is missing, a random UUID kept in the app's preferences. |
| iOS | A random UUID kept in the Keychain (`com.anilili.ios.usage`, this device only, not synced). The Keychain outlives the app; re-signing under another Apple ID loses it. |
| Windows | `MachineGuid` under `HKLM\SOFTWARE\Microsoft\Cryptography`. |
| macOS | `IOPlatformUUID` (from `ioreg`). |
| Linux | `/etc/machine-id`, else `/var/lib/dbus/machine-id`. |

When no raw ID can be read, the app falls back to a random UUID kept in its own storage (counted
as a new device after a reinstall). The salt makes the ID useless outside Anilili: it matches
nothing another app or service sees.

How the platform is chosen:

- **Android**: `android-tv` on a television (the app's own TV detection, either flavor);
  otherwise `android-tablet` when the smallest screen width is 600 dp or more; otherwise `android`.
- **iOS**: `ipad` when the interface idiom is the iPad's, else `iphone`.
- **Desktop**: the operating system it was built for.

### `people`: one person's devices

For each signed-in list service, in the order AniList, MAL, Simkl: the lowercase hex SHA-256 of
`"anilili-person-v1:" + service + ":" + <the service's user ID>`, where the service is `anilist`,
`mal` or `simkl` and the user ID is the account's numeric ID as the service gives it (AniList's
`Viewer.id`, MAL's user `id`, Simkl's account `id`), as a decimal string. Each app's tests check
the same value: `sha256("anilili-person-v1:anilist:123456")` =
`79f2ba9972b3e2f1fb4ee6ce0e47fe7ca11faa2c8d7d435eed44eac40f938860`.

The same account gives the same hash on every device, so the dashboard can see that one person
uses a phone, a TV and a PC. The server hashes it again under its secret key before storing it, so
a stored value can't be checked against a known account. Never the user name, never a token.

### `env`: the device

| Field | Android | iOS | Windows | macOS | Linux |
| --- | --- | --- | --- | --- | --- |
| `os` | `Build.VERSION.RELEASE` (`14`) | `UIDevice.current.systemVersion` (`18.0`) | the version and build (`10.0.22631`) | `ProcessInfo` version (`15.0`) | `/etc/os-release`: `NAME VERSION_ID` (`Ubuntu 24.04`) |
| `api` | `Build.VERSION.SDK_INT` (`34`) | absent | absent | absent | absent |
| `maker` | `Build.MANUFACTURER` | `Apple` | SMBIOS `SystemManufacturer` (`HKLM\HARDWARE\DESCRIPTION\System\BIOS`) | `Apple` | `/sys/class/dmi/id/sys_vendor` |
| `model` | `Build.MODEL` (`AFTKA`, `SM-S911B`) | the hardware model (`utsname.machine`: `iPhone15,2`) | SMBIOS `SystemProductName` | `sysctl hw.model` (`Mac14,2`) | `/sys/class/dmi/id/product_name` |
| `screen` | the display's real size in pixels | `UIScreen.main.nativeBounds` | the primary monitor | the main screen, in pixels | the primary monitor |
| `lang` | the system's first locale | `Locale.preferredLanguages[0]` | the user's UI language | the first preferred language | `LANG` |
| `app_lang` | the language the app shows | the same | the same | the same | the same |

- `screen` is `<width>x<height>`, the way the device is normally held: phones and tablets
  portrait (`1080x2400`), TVs and computers landscape (`1920x1080`).
- `lang` and `app_lang` are BCP 47 tags (`sq-XK`, `en`), with no `_` and no encoding suffix
  (`LANG=sq_AL.UTF-8` is sent as `sq-AL`).
- Text fields are trimmed; at most 24 characters for `os`, 40 for `maker`, 60 for `model`, 20 for
  the languages.

### `settings`: the app's main settings

Lowercase keys, as the app stores them; letters, digits and `-` only.

| Field | Values |
| --- | --- |
| `quality` | The default video quality: `auto`, or the height (`1080`, `720`, `480`, `360`). |
| `theme` | The theme: `dark`, `light`, `system`, or the app's own name for it (`amoled`). |
| `audio` | `sub` or `dub`: the default. |
| `player` | The player: `internal`, `external`, or the app's name for it (`mpv`, `vlc`). |

### `usage`: since the last accepted ping

Counters the app keeps in its own storage from one accepted ping to the next, sent with the next
ping and reset after its `2xx`. The server files them under the day of the previous ping, so a
ping on Tuesday reports Monday.

| Field | Value |
| --- | --- |
| `minutes` | Whole minutes the app was in the foreground (0 to 2880). |
| `watched` | Of those, whole minutes a video was playing. |
| `sessions` | Times the app came to the foreground: a launch, or a return after 5 minutes or more away. |

On a device's first ping (`first`), send what was counted so far (usually zeros). Like
`usage_ping_day`, the counters are this install's own and never go into a backup.

## When

- At most one **accepted** ping per UTC day. After a `2xx`, store today's UTC date
  (`yyyy-MM-dd`) as `usage_ping_day`; don't ping again while it equals today.
- Try when the app comes to the foreground (launch and return), and on the desktop also once an
  hour while it runs, so a day change is noticed. A failure is silent and retried next time.
- `first` is true when `usage_ping_day` is missing.
- **Only from release builds**, so development doesn't count: not Android's debug build, nor
  the `benchmarkRelease`/`nonMinifiedRelease` types the baseline-profile plugin adds (Android
  checks for build type `release`); not a desktop debug build (`tauri dev`); not an iOS Debug
  configuration. The developer's own machines running release builds are flagged as dev devices
  on the dashboard, which keeps their history and counts them nowhere.

The server keeps one record per device per day, so a repeated ping the same day changes nothing.
From the device's earlier days it works out whether a ping is the device's first day of the month
(monthly users), a new device, a reinstall (`first` from a device it already knows) or a return.

## Always on, and said so

From version 2 the ping has no switch: every release build sends it. Version 1's setting
(Settings → App → **Anonymous usage count**, `usage_stats`) goes; an app still on version 1 keeps
it until it moves to version 2. The data is no longer anonymous in the plain sense (`people` links
one person's devices), so the app must not call it that.

The Android, desktop and iOS apps used to state what is sent in their Settings (**Usage
statistics**); that row was removed from all three on 2026-10-22, so no app shows it any more.
The wording they showed was:

> **Usage statistics.** Once a day the app sends a device ID; the device's type, maker, model,
> system and screen; the app's version, language and main settings; how long the app was open and
> how long you watched since the last time; and which list services are signed in, as codes that
> match your other devices signed in to the same accounts. The server adds the country and city
> of your connection and a code for the network it came from, never the address itself.

`usage_ping_day` and the `usage` counters are bookkeeping for this install only: they are
**never** written to a backup, or a restore onto a new device would make it skip a day and miss
its `first`.

## Reading the counts

The dashboard is `https://pulse.anilili.workers.dev/dash` (it asks for the stats key). The
Worker, its database and the keys live outside the app repositories.
