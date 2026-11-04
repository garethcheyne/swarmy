import type { GlobalStats, ServerInfo } from "@/lib/api"
import { formatBytes } from "@/lib/format"

/** The bottom strip: network facts at a glance. Desktop only. */
export function StatusBar({ info, stats }: { info: ServerInfo | null; stats: GlobalStats | undefined }) {
  const net = info?.network
  const disk = stats?.system
  return (
    <footer className="data hidden h-7 shrink-0 items-center gap-6 border-t bg-sidebar px-4 text-[11px] text-sidebar-foreground/80 md:flex">
      {net?.dht_nodes != null && <span>DHT {net.dht_nodes} nodes</span>}
      {net?.listen_port != null && <span>Listening on {net.listen_port}, TCP and uTP</span>}
      <span>{net?.bind_interface ? `Bound to ${net.bind_interface}` : "Not bound to an interface"}</span>
      {disk && disk.disk_total > 0 && (
        <span className={disk.disk_free / disk.disk_total < 0.1 ? "text-destructive" : undefined}>
          Disk {formatBytes(disk.disk_free)} free
        </span>
      )}
      <span>{stats?.peers ?? 0} peers</span>
      <span className="ml-auto">{info ? `Swarmy ${info.version}${info.demo ? ", demo mode" : ""}` : ""}</span>
    </footer>
  )
}
