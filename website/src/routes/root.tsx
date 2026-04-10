import { Outlet, Link } from '@tanstack/react-router'

export function RootLayout() {
  return (
    <div className="min-h-screen bg-background text-foreground">
      <header className="border-b border-border">
        <div className="max-w-4xl mx-auto px-6 py-4 flex items-center justify-between">
          <Link to="/" className="text-myth-blue font-bold text-lg tracking-tight hover:opacity-80 transition-opacity">
            mythcode
          </Link>
          <nav className="flex items-center gap-6">
            <a
              href="https://github.com/lassejlv/mythcode"
              target="_blank"
              rel="noopener noreferrer"
              className="text-muted-foreground hover:text-foreground transition-colors text-sm"
            >
              GitHub
            </a>
            <Link
              to="/releases"
              className="text-muted-foreground hover:text-foreground transition-colors text-sm"
            >
              Releases
            </Link>
          </nav>
        </div>
      </header>
      <main>
        <Outlet />
      </main>
    </div>
  )
}
