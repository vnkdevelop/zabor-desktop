import { unpackGif, getStaticFrameSync } from '../../utils/avatar';


export function AvatarImg({ src, size, bgColor, animate = true, className = '' }: {
  src: string | null | undefined;
  size: number;
  bgColor?: string;
  animate?: boolean;
  className?: string;
}) {
  const containerStyle: React.CSSProperties = {
    WebkitMaskImage: 'radial-gradient(circle, white calc(100% - 0.5px), transparent 100%)',
    maskImage: 'radial-gradient(circle, white calc(100% - 0.5px), transparent 100%)',
    backgroundColor: !src ? bgColor : 'transparent',
    WebkitBackfaceVisibility: 'hidden',
    WebkitTransform: 'translate3d(0, 0, 0)',
  };

  const renderContent = () => {
    if (!src) return null;

    const packed = unpackGif(src);


    if (packed && animate) {
      return (
        <div className="absolute inset-0 pointer-events-none">
          <img
            src={packed.g}
            draggable={false}
            className="absolute left-1/2 top-1/2"
            style={{
              transform: `translate(calc(-50% + ${packed.x * (size / 200)}px), calc(-50% + ${packed.y * (size / 200)}px)) scale(${packed.s})`,
              maxWidth: '100%',
              maxHeight: '100%',
              objectFit: 'contain',
            }}
          />
        </div>
      );
    }


    const staticSrc = (packed && !animate) ? getStaticFrameSync(src) : src;
    if (!staticSrc) return null;

    return (
      <img
        src={staticSrc}
        className="absolute inset-0 w-full h-full object-cover"
        draggable={false}
      />
    );
  };

  return (
    <div
      className={`w-full h-full rounded-full overflow-hidden relative shrink-0 ${className}`}
      style={containerStyle}
    >
      {renderContent()}
    </div>
  );
}
