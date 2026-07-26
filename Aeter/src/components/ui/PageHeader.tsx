export default function PageHeader({
  title,
  subtitle,
  actions
}: {
  title: string
  subtitle?: string
  actions?: React.ReactNode
}): React.JSX.Element {
  return (
    <header className="page-header flex shrink-0 flex-wrap items-end justify-between gap-x-4 gap-y-2 px-[var(--content-x)] pb-5 pt-12">
      <div className="min-w-0">
        <h1
          className="page-title truncate font-extrabold"
          data-text={title}
          style={{
            fontSize: 'var(--page-title-size, clamp(24px, 3.2cqw, 40px))',
            letterSpacing: '-0.03em',
            lineHeight: 1.15
          }}
        >
          {title}
        </h1>
        {subtitle && <p className="page-subtitle mt-0.5 text-[12.5px] text-text-3">{subtitle}</p>}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </header>
  )
}
