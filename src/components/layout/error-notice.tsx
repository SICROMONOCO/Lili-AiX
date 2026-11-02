import { Link } from "@tanstack/react-router"
import { ChevronDownIcon, FileTextIcon, LogInIcon, RefreshCwIcon } from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible"
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { saveDiagnostics, toFailure } from "@/lib/failure"

/**
 * A failure in place of a page's content: the catalogue message, the code to quote, the next
 * step as a button, and the technical detail behind "Details".
 */
export function ErrorNotice({ error, title, onRetry }: { error: unknown; title?: string; onRetry?: () => void }) {
  const failure = toFailure(error)
  return (
    <Empty>
      <EmptyHeader>
        <EmptyTitle>{title ?? failure.message}</EmptyTitle>
        <EmptyDescription className="flex flex-col items-center gap-2">
          {title && <span>{failure.message}</span>}
          {failure.code && (
            <Badge variant="outline" className="font-mono">
              {failure.code}
            </Badge>
          )}
        </EmptyDescription>
      </EmptyHeader>
      <EmptyContent className="flex flex-col items-center gap-3">
        <div className="flex flex-wrap justify-center gap-2">
          {onRetry && (
            <Button onClick={onRetry}>
              <RefreshCwIcon data-icon="inline-start" />
              Try again
            </Button>
          )}
          {failure.next === "signIn" && (
            <Button variant="outline" nativeButton={false} render={<Link to="/settings" />}>
              <LogInIcon data-icon="inline-start" />
              Sign in
            </Button>
          )}
          {failure.next === "diagnostics" && (
            <Button variant="outline" onClick={() => saveDiagnostics()}>
              <FileTextIcon data-icon="inline-start" />
              Save diagnostics
            </Button>
          )}
        </div>
        {failure.detail && (
          <Collapsible className="flex w-full max-w-md flex-col items-center gap-2">
            <CollapsibleTrigger render={<Button variant="ghost" size="sm" />}>
              Details
              <ChevronDownIcon data-icon="inline-end" />
            </CollapsibleTrigger>
            <CollapsibleContent className="w-full">
              <pre className="max-h-40 overflow-auto rounded-md bg-muted p-3 text-left text-xs whitespace-pre-wrap text-muted-foreground select-text">
                {failure.detail}
              </pre>
            </CollapsibleContent>
          </Collapsible>
        )}
      </EmptyContent>
    </Empty>
  )
}
