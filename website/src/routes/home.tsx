import { Badge } from '@/components/ui/badge'
import { Card, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'

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

const installCommands = [
  { label: 'npm', cmd: 'npm install -g @mythcode/cli' },
  { label: 'bun', cmd: 'bun install -g @mythcode/cli' },
]

export function HomePage() {
  return (
    <div className="max-w-4xl mx-auto px-6">
      {/* Hero */}
      <section className="py-24 text-center">
        <Badge variant="secondary" className="mb-6 text-myth-blue border-myth-blue/20 bg-myth-blue/10">
          v0.1.3
        </Badge>
        <h1 className="text-4xl md:text-6xl font-bold tracking-tighter mb-4">
          <span className="text-myth-blue">myth</span>
          <span className="text-foreground">code</span>
        </h1>
        <p className="text-muted-foreground text-lg max-w-lg mx-auto mb-10">
          A fast terminal client for ACP-compatible coding agents.
          Built in Rust.
        </p>
        <div className="flex justify-center gap-3">
          <a href="https://github.com/lassejlv/mythcode" target="_blank" rel="noopener noreferrer">
            <Button size="lg" className="bg-myth-blue text-myth-base hover:bg-myth-blue/90 font-semibold">
              View on GitHub
            </Button>
          </a>
          <a href="https://github.com/lassejlv/mythcode/releases" target="_blank" rel="noopener noreferrer">
            <Button size="lg" variant="outline" className="border-border text-foreground hover:bg-secondary font-semibold">
              Download
            </Button>
          </a>
        </div>
      </section>

      {/* Install */}
      <section className="pb-20">
        <div className="space-y-3">
          {installCommands.map((item) => (
            <div
              key={item.label}
              className="flex items-center gap-4 bg-myth-crust border border-border rounded-lg px-5 py-3 font-mono text-sm"
            >
              <span className="text-muted-foreground w-8 shrink-0">{item.label}</span>
              <span className="text-myth-green">$</span>
              <code className="text-foreground">{item.cmd}</code>
            </div>
          ))}
        </div>
      </section>

      {/* Features */}
      <section className="pb-24">
        <div className="grid md:grid-cols-2 gap-4">
          {features.map((feature) => (
            <Card key={feature.title} className="bg-myth-crust border-border hover:border-myth-blue/30 transition-colors">
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
        <div className="bg-myth-crust border border-border rounded-lg p-6 font-mono text-sm space-y-1">
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
