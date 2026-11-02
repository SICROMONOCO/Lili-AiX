import { Link, useRouter } from "@tanstack/react-router"
import { HomeIcon, RefreshCwIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty"

/** Any render error on a page: say so, and offer a way out that does not need a restart. */
export function RouteError({ error }: { error: unknown }) {
  const router = useRouter()
  const message = error instanceof Error ? error.message : String(error)
  return (
    <div className="flex h-svh items-center justify-center bg-background">
      <Empty>
        <EmptyHeader>
          <EmptyTitle>Something went wrong on this page</EmptyTitle>
          <EmptyDescription>{message} (DAPP-02)</EmptyDescription>
        </EmptyHeader>
        <EmptyContent className="flex-row justify-center gap-2">
          <Button onClick={() => void router.invalidate()}>
            <RefreshCwIcon data-icon="inline-start" />
            Try again
          </Button>
          <Button variant="outline" nativeButton={false} render={<Link to="/" />}>
            <HomeIcon data-icon="inline-start" />
            Home
          </Button>
        </EmptyContent>
      </Empty>
    </div>
  )
}

export function NotFound() {
  return (
    <div className="flex h-svh items-center justify-center bg-background">
      <Empty>
        <EmptyHeader>
          <EmptyTitle>Nothing here</EmptyTitle>
          <EmptyDescription>This page does not exist.</EmptyDescription>
        </EmptyHeader>
        <EmptyContent>
          <Button nativeButton={false} render={<Link to="/" />}>
            Home
          </Button>
        </EmptyContent>
      </Empty>
    </div>
  )
}
