import { useEffect, useState } from "react";
import { IconBooks, IconMusic } from "./NavIcons";

type Props = {
  src: string | null;
  kind?: "audiobook" | "music" | string;
  className?: string;
  alt?: string;
};

export function CoverImage({ src, kind = "audiobook", className, alt = "" }: Props) {
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setFailed(false);
  }, [src]);

  const showFallback = !src || failed;

  if (showFallback) {
    return (
      <div
        className={`cover-fallback${className ? ` ${className}` : ""}`}
        aria-hidden={alt === "" ? true : undefined}
        role={alt ? "img" : undefined}
        aria-label={alt || undefined}
      >
        {kind === "music" ? (
          <IconMusic className="cover-fallback-icon" />
        ) : (
          <IconBooks className="cover-fallback-icon" />
        )}
      </div>
    );
  }

  return (
    <img
      className={className}
      src={src}
      alt={alt}
      decoding="async"
      onError={() => setFailed(true)}
    />
  );
}
