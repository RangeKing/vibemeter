export function capabilityTranslationKey(level: string, available: boolean): string {
  if (!available) return "sources.capabilities.unavailable";
  if (level === "full") return "sources.capabilities.full";
  if (level === "partial") return "sources.capabilities.partial";
  return "sources.capabilities.basic";
}
