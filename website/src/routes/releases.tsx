import { useEffect, useState } from 'react'

import { Card, CardContent } from '@/components/ui/card'
import { Button } from '@/components/ui/button'

interface Release {
  tag_name: string
  name: string
  body: string
  html_url: string
  published_at: string
  prerelease: boolean
}

function formatDate(iso: string) {
  return new Date(iso).toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  })
}

function ReleaseCard({ release }: { release: Release }) {
  const isLatest = release === releases[0]

  return (
    <Card className="bg-myth-mantle border-border hover:border-myth-blue/30 transition-colors">
      <CardContent className="pt-6">
        <div className="flex items-start justify-between gap-4 mb-3">
          <div className="flex items-center gap-3">
            <h3 className="font-semibold text-foreground text-lg">
              <a href={release.html_url} target="_blank" rel="noopener noreferrer" className="hover:text-myth-blue transition-colors">
                {release.name || release.tag_name}
              </a>
            </h3>
            {isLatest && (
              <span className="text-xs font-semibold bg-myth-blue text-myth-crust px-2 py-0.5 rounded-full">
                latest
              </span>
            )}
            {release.prerelease && (
              <span className="text-xs font-semibold bg-myth-peach/20 text-myth-peach px-2 py-0.5 rounded-full">
                pre-release
              </span>
            )}
          </div>
          <span className="text-muted-foreground text-sm shrink-0">{formatDate(release.published_at)}</span>
        </div>
        {release.body && (
          <div className="text-muted-foreground text-sm leading-relaxed whitespace-pre-wrap border-t border-border pt-3">
            {release.body}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

let releases: Release[] = []

export function ReleasesPage() {
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [data, setData] = useState<Release[]>([])

  useEffect(() => {
    fetch('https://api.github.com/repos/lassejlv/mythcode/releases')
      .then(r => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`)
        return r.json()
      })
      .then((json: Release[]) => {
        releases = json
        setData(json)
      })
      .catch(err => setError(err.message))
      .finally(() => setLoading(false))
  }, [])

  return (
    <div className="max-w-4xl mx-auto px-6">
      <section className="py-16">
        <div className="flex items-center justify-between mb-8">
          <h1 className="text-3xl font-bold tracking-tight">
            <span className="text-myth-blue">Releases</span>
          </h1>
          <a href="https://github.com/lassejlv/mythcode/releases" target="_blank" rel="noopener noreferrer">
            <Button variant="outline" size="sm" className="border-border text-muted-foreground hover:text-foreground">
              View on GitHub
            </Button>
          </a>
        </div>

        {loading && (
          <div className="text-center py-16 text-muted-foreground">Loading releases…</div>
        )}

        {error && (
          <Card className="bg-myth-mantle border-myth-red/30">
            <CardContent className="pt-6 text-myth-red">
              Failed to load releases: {error}
            </CardContent>
          </Card>
        )}

        {!loading && !error && (
          <div className="space-y-4">
            {data.map(release => (
              <ReleaseCard key={release.tag_name} release={release} />
            ))}
          </div>
        )}
      </section>

      <footer className="border-t border-border py-8 text-center text-sm text-muted-foreground">
        <p>MIT License · Built with Rust</p>
      </footer>
    </div>
  )
}