export function privateSessionReference(agent: string, sourceSessionId: string): string {
  let hash = 0x811c9dc5;
  for (const character of `${agent}:${sourceSessionId}`) {
    hash ^= character.charCodeAt(0);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(16).toUpperCase().padStart(8, "0").slice(0, 6);
}
