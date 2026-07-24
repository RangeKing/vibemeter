import typeBoardUrl from "../assets/vcti/vcti-types-atlas-v2.webp";
import {
  VCTI_GUILD_ACCENT,
  VCTI_GUILD_PREVIEW_TYPE,
  VCTI_TYPE_GUILD,
  VCTI_TYPE_POSITION,
  type VctiGuild,
} from "../lib/vctiCatalog";

export function VctiAvatar({
  type,
  guild,
  size = "large",
  label,
}: {
  type?: string;
  guild?: string;
  size?: "small" | "medium" | "large";
  label?: string;
}) {
  const resolvedGuild = ((type && VCTI_TYPE_GUILD[type]) || guild || "start") as VctiGuild;
  const resolvedType = type || VCTI_GUILD_PREVIEW_TYPE[resolvedGuild];
  return (
    <div
      className={`vcti-avatar ${size}`}
      style={{ "--vcti-accent": VCTI_GUILD_ACCENT[resolvedGuild] } as React.CSSProperties}
      aria-label={label}
      role={label ? "img" : undefined}
    >
      <span
        className="vcti-avatar-art"
        style={{
          backgroundImage: `url(${typeBoardUrl})`,
          backgroundPosition: VCTI_TYPE_POSITION[resolvedType],
        }}
      />
      {type ? <strong className="vcti-avatar-code">{type}</strong> : null}
    </div>
  );
}
