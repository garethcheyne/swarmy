import { useState, type FormEvent } from "react"

import { api } from "@/lib/api"
import { cn } from "@/lib/utils"
import { SwarmCanvas, type SwarmMode } from "@/components/swarm-canvas"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"

/**
 * The front door: a live swarm assembles into the Swarmy S. It tightens while
 * you sign in, bursts apart on a wrong password, and rushes into the S when
 * you're in.
 */
export function Login({ onSuccess }: { onSuccess: () => void }) {
  const [username, setUsername] = useState("admin")
  const [password, setPassword] = useState("")
  const [error, setError] = useState<string | null>(null)
  const [mode, setMode] = useState<SwarmMode>("idle")

  async function submit(e: FormEvent) {
    e.preventDefault()
    setMode("busy")
    setError(null)
    try {
      await api.login(username, password)
      setMode("success")
      // Let the swarm finish rushing in before the app appears.
      const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches
      setTimeout(onSuccess, reduced ? 0 : 650)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
      setMode("error")
      setTimeout(() => setMode("idle"), 900)
    }
  }

  const pending = mode === "busy" || mode === "success"

  return (
    <div className="relative min-h-svh overflow-hidden bg-background">
      <div aria-hidden className="login-aura pointer-events-none absolute inset-0" />
      <SwarmCanvas mode={mode} className="absolute inset-0 size-full" />

      <div className="relative z-10 grid min-h-svh items-end gap-10 lg:items-center px-[max(1.25rem,env(safe-area-inset-left))] py-10 lg:grid-cols-[minmax(0,1fr)_25rem] lg:px-16">
        <div className="pointer-events-none hidden self-end pb-6 lg:block">
          <h1 className="text-[clamp(4rem,9vw,8.5rem)] leading-[0.85] pb-[0.14em] font-bold tracking-[-0.035em] login-wordmark">
            Swarmy
          </h1>
          <p className="mt-5 max-w-md text-lg text-muted-foreground">
            Every piece, from every peer. No ads, no tracking, nothing bundled. Just your swarm.
          </p>
        </div>

        <form
          onSubmit={submit}
          className={cn(
            "mx-auto w-full max-w-sm rounded-2xl border bg-card/85 p-6 shadow-2xl backdrop-blur-md transition-transform lg:mx-0",
            mode === "error" && "motion-safe:animate-[shake_0.4s_ease-in-out]"
          )}
        >
          <div className="mb-6 flex flex-col gap-1">
            <h2 className="login-wordmark pb-[0.1em] text-2xl font-bold tracking-[-0.01em] lg:hidden">Swarmy</h2>
            <h2 className="hidden text-xl font-semibold lg:block">Sign in</h2>
            <p className="text-[13px] text-muted-foreground">
              The first password is printed in Swarmy's log when it starts for the first time.
            </p>
          </div>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="username">Username</FieldLabel>
              <Input id="username" autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} />
            </Field>
            <Field data-invalid={error ? true : undefined}>
              <FieldLabel htmlFor="password">Password</FieldLabel>
              <Input
                id="password"
                type="password"
                autoComplete="current-password"
                autoFocus
                aria-invalid={error ? true : undefined}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
              {error && <FieldError>{error}</FieldError>}
            </Field>
            <Button type="submit" size="lg" disabled={pending}>
              {pending && <Spinner data-icon="inline-start" />}
              {mode === "success" ? "Joining the swarm…" : "Sign in"}
            </Button>
          </FieldGroup>
        </form>
      </div>
    </div>
  )
}
