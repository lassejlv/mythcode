import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { RouterProvider, createRouter, createRootRoute, createRoute } from '@tanstack/react-router'
import './index.css'
import { RootLayout } from './routes/root'
import { HomePage } from './routes/home'
import { ReleasesPage } from './routes/releases'

const rootRoute = createRootRoute({
  component: RootLayout,
})

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: HomePage,
})

const releasesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/releases',
  component: ReleasesPage,
})

const routeTree = rootRoute.addChildren([indexRoute, releasesRoute])
const router = createRouter({ routeTree })

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <RouterProvider router={router} />
  </StrictMode>,
)
