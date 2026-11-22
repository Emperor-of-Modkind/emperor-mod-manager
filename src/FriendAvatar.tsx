import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faUser } from "@fortawesome/free-solid-svg-icons";

function getInitials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return `${parts[0][0]}${parts[1][0]}`.toUpperCase();
}

const MAX_AVATAR_DATA_URL_LEN = 262_144;
const SAFE_AVATAR_DATA_URL_RE =
  /^data:image\/(?:png|jpeg|webp|gif);base64,[A-Za-z0-9+/]+=*$/;

export function isSafeAvatarSrc(src?: string | null): src is string {
  if (!src || src.length > MAX_AVATAR_DATA_URL_LEN) return false;
  return SAFE_AVATAR_DATA_URL_RE.test(src);
}

export function FriendAvatar({
  name,
  src,
  online,
  size = "md",
  showStatus = false,
}: {
  name: string;
  src?: string | null;
  online?: boolean;
  size?: "xs" | "sm" | "md" | "lg";
  showStatus?: boolean;
}) {
  const initials = getInitials(name);
  const safeSrc = isSafeAvatarSrc(src) ? src : null;
  return (
    <span className={`friends-avatar friends-avatar-${size}`}>
      {safeSrc ? (
        <img className="friends-avatar-img" src={safeSrc} alt={name || "Avatar"} />
      ) : (
        <span className="friends-avatar-fallback" aria-hidden="true">
          {initials || <FontAwesomeIcon icon={faUser} />}
        </span>
      )}
      {showStatus ? (
        <span
          className={`friends-avatar-status ${online ? "online" : "offline"}`}
          aria-hidden="true"
        />
      ) : null}
    </span>
  );
}

export async function resizeProfilePicture(
  file: File,
  size = 256,
): Promise<string> {
  const url = URL.createObjectURL(file);
  try {
    const img = await new Promise<HTMLImageElement>((resolve, reject) => {
      const el = new Image();
      el.onload = () => resolve(el);
      el.onerror = () => reject(new Error("Could not load selected image."));
      el.src = url;
    });
    const canvas = document.createElement("canvas");
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      throw new Error("Could not initialize image canvas.");
    }
    const minSide = Math.min(img.naturalWidth || size, img.naturalHeight || size);
    const sx = Math.max(0, ((img.naturalWidth || size) - minSide) / 2);
    const sy = Math.max(0, ((img.naturalHeight || size) - minSide) / 2);
    ctx.drawImage(img, sx, sy, minSide, minSide, 0, 0, size, size);

    const webp = canvas.toDataURL("image/webp", 0.85);
    if (webp.startsWith("data:image/webp;base64,")) {
      return webp;
    }
    return canvas.toDataURL("image/png");
  } finally {
    URL.revokeObjectURL(url);
  }
}
