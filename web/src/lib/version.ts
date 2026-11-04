/** Compare dotted versions ("0.10.2" > "0.9.0"); pre-release tags sort first. */
export function compareVersions(a: string, b: string) {
  const split = (v: string) => {
    const [core, pre = ""] = v.replace(/^v/, "").split("-", 2)
    return { parts: core.split(".").map((n) => parseInt(n, 10) || 0), pre }
  }
  const x = split(a)
  const y = split(b)
  for (let i = 0; i < Math.max(x.parts.length, y.parts.length); i++) {
    const d = (x.parts[i] ?? 0) - (y.parts[i] ?? 0)
    if (d !== 0) return Math.sign(d)
  }
  if (x.pre === y.pre) return 0
  if (!x.pre) return 1
  if (!y.pre) return -1
  return x.pre < y.pre ? -1 : 1
}

const KEY = "swarmy.seen-version"

/** The version this browser last showed release notes for, if any. */
export function seenVersion(): string | null {
  try {
    return localStorage.getItem(KEY)
  } catch {
    return null
  }
}

export function markSeen(version: string) {
  try {
    localStorage.setItem(KEY, version)
  } catch {
    // Private mode or blocked storage: the notes just show again next time.
  }
}
