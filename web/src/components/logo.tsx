/** Swarmy mark: a honey cell with a smaller cell rising inside it. */
export function Logo({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 32 32" className={className} aria-hidden>
      <path d="M16 1.5 28.6 8.75v14.5L16 30.5 3.4 23.25V8.75Z" fill="var(--sidebar-primary)" />
      <path
        d="M16 11.2 21.9 14.6v6.8L16 24.8l-5.9-3.4v-6.8Z"
        fill="var(--sidebar)"
        opacity="0.92"
      />
    </svg>
  )
}
