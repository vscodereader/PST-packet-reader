import { Box } from "@mantine/core";

import bandLogo from "@/assets/logos/band.svg";
import instagramLogo from "@/assets/logos/instagram.svg";
import navercafeLogo from "@/assets/logos/navercafe.svg";
import threadsLogo from "@/assets/logos/threads.svg";
import { PLATFORM } from "@/shared/data/mock";
import type { PlatformId } from "@/shared/data/types";

const INITIAL: Record<PlatformId, string> = {
  forum: "토",
  naver: "N",
  band: "B",
  instagram: "I",
  threads: "@",
};

/** Real brand marks; platforms absent here fall back to the initial badge. */
const LOGO: Partial<Record<PlatformId, string>> = {
  naver: navercafeLogo,
  band: bandLogo,
  instagram: instagramLogo,
  threads: threadsLogo,
};

interface PlatformLogoProps {
  id: PlatformId;
  size?: number;
  dim?: boolean;
}

/** Each platform's brand mark — the real SVG logo, or a colored initial badge. */
export function PlatformLogo({
  id,
  size = 38,
  dim = false,
}: PlatformLogoProps) {
  const p = PLATFORM[id];
  const logo = LOGO[id];

  if (logo) {
    return (
      <img
        src={logo}
        alt={p?.name ?? id}
        width={size}
        height={size}
        style={{
          flexShrink: 0,
          borderRadius: size * 0.32,
          opacity: dim ? 0.5 : 1,
        }}
      />
    );
  }

  return (
    <Box
      style={{
        width: size,
        height: size,
        flexShrink: 0,
        borderRadius: size * 0.32,
        background: `var(--mantine-color-${p?.color ?? "gray"}-6)`,
        color: "#fff",
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        fontSize: size * 0.42,
        fontWeight: 800,
        opacity: dim ? 0.5 : 1,
      }}
    >
      {INITIAL[id]}
    </Box>
  );
}

interface PlatformPillProps {
  ids: PlatformId[];
  size?: number;
}

/** Overlapping stack of platform logos. */
export function PlatformPill({ ids, size = 22 }: PlatformPillProps) {
  return (
    <Box style={{ display: "inline-flex" }}>
      {ids.map((id, i) => (
        <Box
          key={id}
          style={{
            marginLeft: i === 0 ? 0 : -7,
            zIndex: ids.length - i,
            border: "2px solid var(--mantine-color-body)",
            borderRadius: size * 0.32,
            display: "inline-flex",
          }}
        >
          <PlatformLogo id={id} size={size} />
        </Box>
      ))}
    </Box>
  );
}
