import { useEffect, useState } from "react"

/** Fetch on mount and every `ms` milliseconds while mounted. */
export function usePoll<T>(fetcher: () => Promise<T>, ms: number, key: string = "") {
  const [data, setData] = useState<T | null>(null)
  const [error, setError] = useState<Error | null>(null)

  useEffect(() => {
    let cancelled = false
    const load = () =>
      fetcher()
        .then((d) => {
          if (!cancelled) {
            setData(d)
            setError(null)
          }
        })
        .catch((e) => !cancelled && setError(e instanceof Error ? e : new Error(String(e))))
    load()
    const id = setInterval(load, ms)
    return () => {
      cancelled = true
      clearInterval(id)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` identifies the fetcher
  }, [ms, key])

  return { data, error }
}
