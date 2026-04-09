import { Card, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { useState } from 'react'

const features = [
  {
    icon: '◆',
    title: 'Multi-Provider',
    description: 'Connect to Claude, OpenCode, Codex, or Pi agents through the ACP protocol.',
    color: 'text-myth-peach',
  },
  {
    icon: '◇',
    title: 'Interactive TUI',
    description: 'Beautiful terminal interface with streaming responses and syntax highlighting.',
    color: 'text-myth-blue',
  },
  {
    icon: '⌕',
    title: 'Catppuccin Mocha',
    description: 'Gorgeous syntax highlighting with the Catppuccin Mocha color palette.',
    color: 'text-myth-mauve',
  },
  {
    icon: '+',
    title: 'File Context',
    description: 'Reference files with @mentions. See diffs with colored backgrounds.',
    color: 'text-myth-green',
  },
]

const installOptions = [
  { id: 'npm', label: 'npm', cmd: 'npm install -g @mythcode/cli' },
  { id: 'bun', label: 'bun', cmd: 'bun install -g @mythcode/cli' },
  { id: 'binary', label: 'binary', cmd: 'curl -fsSL https://github.com/lassejlv/mythcode/releases/latest/download/install.sh | sh' },
] as const

export function HomePage() {
  const [activeTab, setActiveTab] = useState<string>('npm')
  const [copied, setCopied] = useState(false)

  const activeCmd = installOptions.find(o => o.id === activeTab)!

  function copyCmd() {
    navigator.clipboard.writeText(activeCmd.cmd)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <div className="max-w-4xl mx-auto px-6">
      {/* Hero */}
      <section className="py-28 text-center">
        <h1 className="text-4xl md:text-6xl font-bold tracking-tighter mb-4">
          <span className="text-myth-blue">myth</span>
          <span className="text-foreground">code</span>
        </h1>
        <p className="text-muted-foreground text-lg max-w-md mx-auto mb-12">
          A fast terminal client for ACP-compatible coding agents. Built in Rust.
        </p>
        <div className="flex justify-center gap-3">
          <a href="https://github.com/lassejlv/mythcode" target="_blank" rel="noopener noreferrer">
            <Button size="lg" className="bg-myth-blue text-myth-crust hover:bg-myth-blue/90 font-semibold">
              View on GitHub
            </Button>
          </a>
          <a href="https://github.com/lassejlv/mythcode/releases" target="_blank" rel="noopener noreferrer">
            <Button size="lg" variant="outline" className="border-border text-foreground hover:bg-myth-surface font-semibold">
              Releases
            </Button>
          </a>
        </div>
      </section>

      {/* Install */}
      <section className="pb-24">
        <h2 className="text-2xl font-bold tracking-tight mb-6">Install</h2>
        <div className="bg-myth-mantle border border-border rounded-lg overflow-hidden">
          {/* Tabs */}
          <div className="flex border-b border-border">
            {installOptions.map((opt) => (
              <button
                key={opt.id}
                onClick={() => setActiveTab(opt.id)}
                className={`px-5 py-2.5 text-sm transition-colors ${
                  activeTab === opt.id
                    ? 'text-myth-blue border-b-2 border-myth-blue -mb-px bg-myth-base/50'
                    : 'text-muted-foreground hover:text-foreground'
                }`}
              >
                {opt.label}
              </button>
            ))}
          </div>
          {/* Command */}
          <div className="flex items-center justify-between px-5 py-4">
            <div className="flex items-center gap-3 font-mono text-sm overflow-x-auto">
              <span className="text-myth-green shrink-0">$</span>
              <code className="text-foreground whitespace-nowrap">{activeCmd.cmd}</code>
            </div>
            <button
              onClick={copyCmd}
              className="text-muted-foreground hover:text-foreground transition-colors ml-4 shrink-0 text-sm"
              title="Copy to clipboard"
            >
              {copied ? '✓' : 'copy'}
            </button>
          </div>
        </div>
      </section>

      {/* Features */}
      <section className="pb-24">
        <h2 className="text-2xl font-bold tracking-tight mb-6">Features</h2>
        <div className="grid md:grid-cols-2 gap-4">
          {features.map((feature) => (
            <Card key={feature.title} className="bg-myth-mantle border-border hover:border-myth-blue/30 transition-colors">
              <CardContent className="pt-6">
                <div className="flex items-start gap-3">
                  <span className={`text-xl ${feature.color}`}>{feature.icon}</span>
                  <div>
                    <h3 className="font-semibold text-foreground mb-1">{feature.title}</h3>
                    <p className="text-muted-foreground text-sm leading-relaxed">{feature.description}</p>
                  </div>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      </section>

      {/* Usage */}
      <section className="pb-24">
        <h2 className="text-2xl font-bold tracking-tight mb-6">Usage</h2>
        <div className="bg-myth-mantle border border-border rounded-lg p-6 font-mono text-sm space-y-1">
          <div className="text-muted-foreground"># interactive TUI</div>
          <div><span className="text-myth-green">$</span> mythcode</div>
          <div className="h-3" />
          <div className="text-muted-foreground"># one-shot prompt</div>
          <div><span className="text-myth-green">$</span> mythcode <span className="text-myth-yellow">"explain this function"</span></div>
          <div className="h-3" />
          <div className="text-muted-foreground"># scoped to a directory</div>
          <div><span className="text-myth-green">$</span> mythcode <span className="text-myth-mauve">-p</span> ./my-app <span className="text-myth-yellow">"fix the tests"</span></div>
          <div className="h-3" />
          <div className="text-muted-foreground"># skip provider picker</div>
          <div><span className="text-myth-green">$</span> mythcode <span className="text-myth-mauve">--provider</span> claude</div>
        </div>
      </section>

      {/* Footer */}
      <footer className="border-t border-border py-8 text-center text-sm text-muted-foreground">
        <p>MIT License &middot; Built with Rust</p>
      </footer>
    </div>
  )
}
