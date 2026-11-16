// Line icons (lucide, ISC): only the icons named here end up in the bundle. Static markup, no user text.
import {
  Activity, Bell, Bluetooth, Headphones, Keyboard, Smartphone, CalendarRange, Check, Clipboard, Calendar, ChevronDown, ChevronUp, Circle, CirclePlay, Cloud, CloudDrizzle, CloudFog, CloudLightning,
  CloudRain, CloudSun, Cpu, Ellipsis, ExternalLink, Folder, Globe, HardDrive, House, LayoutGrid, ListTodo, Lock, Volume2, VolumeX, Mail, StickyNote, Trash2, MemoryStick,
  MessageSquareQuote, Mic, Moon, Music, Pause, Pencil, PhoneCall, Pin, Play, Plus, Power, Repeat, RotateCw, Settings, SkipBack, SkipForward, Smile,
  Snowflake, Square, SquareTerminal, Sun, Timer, Zap, Droplets, Wind, Umbrella, Thermometer, Download, type IconNode,
} from "lucide";

const ICONS: Record<string, IconNode> = {
  circle: Circle, music: Music, play: Play, pause: Pause, prev: SkipBack, next: SkipForward,
  bolt: Zap, email: Mail, cpu: Cpu, memory: MemoryStick, disk: HardDrive, web: Globe, open: ExternalLink, pencil: Pencil,
  home: House, plus: Plus, chevronUp: ChevronUp, chevronDown: ChevronDown, timer: Timer, lyrics: MessageSquareQuote, dots: Ellipsis,
  calendar: Calendar, month: CalendarRange, tasks: ListTodo, launcher: LayoutGrid, pal: Smile, pin: Pin, cog: Settings, notes: StickyNote, trash: Trash2,
  bell: Bell, clipboard: Clipboard, check: Check, repeat: Repeat, volume: Volume2, mute: VolumeX, lock: Lock,
  bluetooth: Bluetooth, headphones: Headphones, keyboard: Keyboard, phone: Smartphone, mic: Mic, phoneCall: PhoneCall, stop: Square,
  humidity: Droplets, wind: Wind, umbrella: Umbrella, feels: Thermometer, download: Download,
  // weather keys come from the Rust weather monitor
  sunny: Sun, partly: CloudSun, cloudy: Cloud, fog: CloudFog, rainy: CloudDrizzle, pouring: CloudRain, snowy: Snowflake, lightning: CloudLightning,
  // launcher built-in actions (lucide has no brand icons: chrome → globe, youtube → circle-play)
  power: Power, restart: RotateCw, sleep: Moon, folder: Folder, taskmgr: Activity, chrome: Globe, youtube: CirclePlay, console: SquareTerminal,
};

export function icon(name: string, size = 18): string {
  const body = (ICONS[name] ?? Circle).map(([tag, attrs]) => `<${tag} ${Object.entries(attrs).map(([k, v]) => `${k}="${v}"`).join(" ")}/>`).join("");
  return `<svg viewBox="0 0 24 24" width="${size}" height="${size}" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;
}

/** Fill every `[data-icon]` element under `root` with its SVG. */
export function hydrateIcons(root: ParentNode = document): void {
  root.querySelectorAll<HTMLElement>("[data-icon]").forEach((el) => {
    el.innerHTML = icon(el.dataset.icon!, Number(el.dataset.size ?? 18));
  });
}
