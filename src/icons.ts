// Material Design Icons as inline SVG.
import {
  mdiChartBubble, mdiChevronUp, mdiCircle, mdiClose, mdiCogOutline, mdiConsole, mdiCpu64Bit, mdiEmail, mdiFolderOutline,
  mdiGoogleChrome, mdiHarddisk, mdiHome, mdiLightningBolt, mdiMemory, mdiMusic, mdiOpenInNew, mdiPause,
  mdiPencil, mdiPlay, mdiPlus, mdiPower, mdiRefresh, mdiSkipNext, mdiSkipPrevious, mdiSleep, mdiSpeedometer, mdiTimerOutline,
  mdiWeatherCloudy, mdiWeatherFog, mdiWeatherLightning, mdiWeatherPartlyCloudy, mdiWeatherPouring,
  mdiWeatherRainy, mdiWeatherSnowy, mdiWeatherSunny, mdiWeb, mdiYoutube,
} from "@mdi/js";

const ICONS: Record<string, string> = {
  circle: mdiCircle, music: mdiMusic, play: mdiPlay, pause: mdiPause, prev: mdiSkipPrevious, next: mdiSkipNext,
  speed: mdiSpeedometer, bolt: mdiLightningBolt, email: mdiEmail, cpu: mdiCpu64Bit, memory: mdiMemory,
  disk: mdiHarddisk, web: mdiWeb, open: mdiOpenInNew, pencil: mdiPencil,
  home: mdiHome, plus: mdiPlus, chevronUp: mdiChevronUp, timer: mdiTimerOutline, close: mdiClose,
  // weather keys come from the Rust weather monitor
  sunny: mdiWeatherSunny, partly: mdiWeatherPartlyCloudy, cloudy: mdiWeatherCloudy, fog: mdiWeatherFog,
  rainy: mdiWeatherRainy, pouring: mdiWeatherPouring, snowy: mdiWeatherSnowy, lightning: mdiWeatherLightning,
  // launcher built-in actions
  power: mdiPower, restart: mdiRefresh, sleep: mdiSleep, folder: mdiFolderOutline, cog: mdiCogOutline,
  taskmgr: mdiChartBubble, chrome: mdiGoogleChrome, youtube: mdiYoutube, console: mdiConsole,
};

export function icon(name: string, size = 18, color = "currentColor"): string {
  const d = ICONS[name] ?? ICONS.circle;
  return `<svg viewBox="0 0 24 24" width="${size}" height="${size}" aria-hidden="true"><path fill="${color}" d="${d}"/></svg>`;
}

/** Fill every `[data-icon]` element under `root` with its SVG. */
export function hydrateIcons(root: ParentNode = document): void {
  root.querySelectorAll<HTMLElement>("[data-icon]").forEach((el) => {
    el.innerHTML = icon(el.dataset.icon!, Number(el.dataset.size ?? 18));
  });
}
