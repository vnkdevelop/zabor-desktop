export function Md3Switch({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void; }) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={`relative w-[52px] h-[32px] rounded-full transition-colors duration-200 shrink-0 border-2 focus:outline-none
        ${checked ? 'bg-[#c70060] border-[#c70060]' : 'bg-transparent border-[#79747E]'}`}
    >
      <span
        style={{ willChange: 'transform' }}
        className={`absolute top-[2px] left-[2px] w-6 h-6 rounded-full transition-transform duration-200 ease-out flex items-center justify-center
          ${checked ? 'translate-x-[20px] scale-1 bg-white shadow-md' : 'translate-x-0 scale-[0.66] bg-[#79747E]'}`}
      >
        {checked && (
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#c70060" strokeWidth="3.5" strokeLinecap="round" strokeLinejoin="round">
            <polyline points="20 6 9 17 4 12" />
          </svg>
        )}
      </span>
    </button>
  );
}
