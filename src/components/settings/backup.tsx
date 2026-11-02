// Backup and restore, in the file format the Android app reads and writes (src-tauri/src/backup.rs).

import { useQueryClient } from "@tanstack/react-query"
import { DownloadIcon, UploadIcon } from "lucide-react"
import { useRef, useState } from "react"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"
import { SettingsGroup } from "@/components/settings/section"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldContent, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import { Switch } from "@/components/ui/switch"
import { api, useOpenSubtitles, useViewer } from "@/lib/api"
import { toastFailure } from "@/lib/failure"
import type { BackupInfo } from "@/lib/types"

const SERVICES: Record<string, string> = { anilist: "AniList", mal: "MyAnimeList", opensubtitles: "OpenSubtitles" }

export function Backup() {
  const anilist = !!useViewer().data
  const openSubtitles = !!useOpenSubtitles().data?.username
  // The logins a backup can carry: AniList's token and the OpenSubtitles account, as Android's do.
  const logins = [anilist && "AniList", openSubtitles && "OpenSubtitles"].filter(Boolean).join(" and ")
  const signedIn = logins !== ""
  const [withLogin, setWithLogin] = useState(false)
  const [passphrase, setPassphrase] = useState("")
  const [exporting, setExporting] = useState(false)
  const [pending, setPending] = useState<{ text: string; info: BackupInfo } | null>(null)
  const file = useRef<HTMLInputElement>(null)

  const includeLogin = async (on: boolean) => {
    setWithLogin(on)
    if (on && !passphrase) setPassphrase(await api.backupSuggestPassphrase())
  }

  const exportBackup = async () => {
    setExporting(true)
    try {
      const done = await api.backupExport(signedIn && withLogin ? passphrase : null)
      toast.success("Backup saved to Downloads", {
        description: done.summary,
        action: { label: "Show", onClick: () => void api.revealPath(done.path) },
      })
    } catch (e) {
      toastFailure(e)
    } finally {
      setExporting(false)
    }
  }

  const pick = async (picked: File | undefined) => {
    if (!picked) return
    try {
      const text = await picked.text()
      setPending({ text, info: await api.backupInspect(text) })
    } catch (e) {
      toastFailure(e)
    }
  }

  return (
    <SettingsGroup title="Backup" description="Export or restore, in the file the Android app uses">
        {signedIn && (
          <FieldGroup>
            <Field orientation="horizontal">
              <FieldContent>
                <FieldLabel>Include the {logins} login{logins.includes(" and ") ? "s" : ""}</FieldLabel>
                <FieldDescription>Sealed with a passphrase. You'll need it to restore them.</FieldDescription>
              </FieldContent>
              <Switch checked={withLogin} onCheckedChange={(on) => void includeLogin(on)} aria-label="Include the logins" />
            </Field>
            {withLogin && (
              <Field>
                <FieldLabel htmlFor="backup-passphrase">Passphrase</FieldLabel>
                <Input
                  id="backup-passphrase"
                  value={passphrase}
                  onChange={(e) => setPassphrase(e.target.value)}
                  spellCheck={false}
                  className="font-mono"
                />
                <FieldDescription>Write it down: it isn't stored anywhere, and the login can't be opened without it.</FieldDescription>
              </Field>
            )}
          </FieldGroup>
        )}
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" loading={exporting} disabled={signedIn && withLogin && !passphrase.trim()} onClick={() => void exportBackup()}>
            {exporting ? <Spinner data-icon="inline-start" /> : <DownloadIcon data-icon="inline-start" />}
            Export backup
          </Button>
          <Button variant="outline" onClick={() => file.current?.click()}>
            <UploadIcon data-icon="inline-start" />
            Restore from a file…
          </Button>
          <input
            ref={file}
            type="file"
            accept=".json,application/json"
            className="hidden"
            onChange={(e) => {
              void pick(e.target.files?.[0])
              e.target.value = ""
            }}
          />
        </div>
      {pending && <RestoreDialog text={pending.text} info={pending.info} onClose={() => setPending(null)} />}
    </SettingsGroup>
  )
}

function RestoreDialog({ text, info, onClose }: { text: string; info: BackupInfo; onClose: () => void }) {
  const client = useQueryClient()
  const [passphrase, setPassphrase] = useState("")
  const [restoring, setRestoring] = useState(false)
  const sealed = info.services.map((s) => SERVICES[s] ?? s)

  const restore = async () => {
    setRestoring(true)
    try {
      const done = await api.backupRestore(text, passphrase.trim() || null)
      await client.invalidateQueries()
      toast.success("Backup restored", { description: done.summary })
      for (const warning of done.warnings) toast.warning(warning)
      onClose()
    } catch (e) {
      toastFailure(e)
    } finally {
      setRestoring(false)
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && !restoring && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Restore this backup?</DialogTitle>
          <DialogDescription>
            From {info.appVersion.startsWith("desktop") ? "Anilili Desktop" : "the Android app"},{" "}
            {new Date(info.createdAt).toLocaleString()}: {info.summary}.
          </DialogDescription>
        </DialogHeader>
        <p className="text-sm text-muted-foreground">
          Its settings replace these. Saved titles and progress are added to what's here: nothing is removed, and newer
          progress on this computer is kept.
        </p>
        {sealed.length > 0 && (
          <Field>
            <FieldLabel htmlFor="restore-passphrase">Passphrase for the {sealed.join(" and ")} login</FieldLabel>
            <Input
              id="restore-passphrase"
              type="password"
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
              placeholder="Leave empty to skip the logins"
              autoFocus
            />
          </Field>
        )}
        <DialogFooter>
          <Button variant="outline" disabled={restoring} onClick={onClose}>
            Cancel
          </Button>
          <Button loading={restoring} onClick={() => void restore()}>
            {restoring && <Spinner data-icon="inline-start" />}
            Restore
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
