export interface PackedGif {
  g: string;
  s: number;
  x: number;
  y: number;
}

export function isPackedGif(src: string | null | undefined): boolean {
  return !!src && src.charAt(0) === '{' && src.includes('"g"');
}

export function packGif(gifDataUrl: string, scale: number, x: number, y: number): string {
  return JSON.stringify({
    g: gifDataUrl,
    s: Math.round(scale * 1000) / 1000,
    x: Math.round(x * 10) / 10,
    y: Math.round(y * 10) / 10
  });
}

export function unpackGif(src: string): PackedGif | null {
  if (!isPackedGif(src)) return null;
  try {
    const d = JSON.parse(src);
    return { g: d.g, s: d.s ?? 1, x: d.x ?? 0, y: d.y ?? 0 };
  } catch {
    return null;
  }
}


export function getDisplaySrc(avatarBase64: string | null | undefined): string | null {
  if (!avatarBase64) return null;
  const packed = unpackGif(avatarBase64);
  return packed ? packed.g : avatarBase64;
}


const staticCache = new Map<string, string>();

export function getStaticFrameSync(src: string): string | null {
  return staticCache.get(src.substring(0, 120)) || null;
}

export async function getStaticFrame(src: string): Promise<string> {
  const key = src.substring(0, 120);
  if (staticCache.has(key)) return staticCache.get(key)!;

  const packed = unpackGif(src);
  const imgSrc = packed ? packed.g : src;

  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      const canvas = document.createElement('canvas');
      canvas.width = 200;
      canvas.height = 200;
      const ctx = canvas.getContext('2d')!;

      if (packed) {
        const ratio = Math.min(200 / img.naturalWidth, 200 / img.naturalHeight);
        const baseW = img.naturalWidth * ratio;
        const baseH = img.naturalHeight * ratio;
        ctx.fillStyle = '#000';
        ctx.fillRect(0, 0, 200, 200);
        ctx.translate(100, 100);
        ctx.scale(packed.s, packed.s);
        ctx.translate(packed.x / packed.s, packed.y / packed.s);
        ctx.drawImage(img, -baseW / 2, -baseH / 2, baseW, baseH);
      } else {
        ctx.drawImage(img, 0, 0, 200, 200);
      }

      const png = canvas.toDataURL('image/png');
      staticCache.set(key, png);
      resolve(png);
    };
    img.onerror = () => resolve('');
    img.src = imgSrc;
  });
}

export function preloadStaticFrame(src: string | null | undefined): void {
  if (src && isPackedGif(src)) getStaticFrame(src);
}