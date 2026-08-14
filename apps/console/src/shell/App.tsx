/**
 * The Kontor operator console.
 *
 * One realm at a time, reached over one authenticated `/v1` contract, with the
 * top bar always saying which realm it is and how current what is on screen
 * actually is.
 *
 * There is no realm switcher that merges, no account picker and no view that
 * aggregates across realms: every cache is keyed by `(realm_id, aggregate_id)`,
 * and attaching to another realm starts a new projection rather than adding to
 * this one.
 */
import { useEffect, useState } from 'react'
import type { Endpoint } from '../api/endpoint'
import { browserStore, inDesktop, strongholdStore, type CredentialStore } from './credentials'
import { Connect } from './Connect'
import { TopBar } from './TopBar'
import { NavRail, type ViewId } from './NavRail'
import { useRealm } from './useRealm'
import { BoardView } from '../views/BoardView'
import { TaskView } from '../views/TaskView'
import { SessionView } from '../views/SessionView'
import { IntakeView, ScheduleView, WorkflowView } from '../views/GatedViews'
import { TeamsView } from '../views/TeamsView'

/** Render the console. */
export function App({ store }: { store?: CredentialStore }) {
  const [resolved, setResolved] = useState<CredentialStore | null>(store ?? null)
  const [endpoint, setEndpoint] = useState<Endpoint | null>(null)
  const [view, setView] = useState<ViewId>('board')
  const [selectedRun, setSelectedRun] = useState<string | null>(null)
  const [selectedTask, setSelectedTask] = useState<{ projectId: string; taskId: string } | null>(
    null,
  )

  // Which store is available depends on where this is running, and that is only
  // knowable asynchronously inside the desktop shell.
  useEffect(() => {
    if (store) {
      return
    }
    let live = true
    void inDesktop().then((desktop) => {
      if (live) {
        setResolved(desktop ? strongholdStore : browserStore)
      }
    })
    return () => {
      live = false
    }
  }, [store])

  const realm = useRealm(endpoint)

  if (!resolved) {
    return <main className="loading">Starting…</main>
  }
  if (!endpoint) {
    return <Connect store={resolved} onConnect={setEndpoint} />
  }

  return (
    <div className="console" data-view={view}>
      <a className="skip-link" href="#view">
        Skip to the view
      </a>
      <TopBar
        endpoint={endpoint}
        realm={realm.realm}
        health={realm.health}
        control={realm.control}
      />
      <NavRail current={view} onSelect={setView} />
      <main id="view" className="view-host" tabIndex={-1}>
        {realm.error ? (
          <p className="banner" role="alert" data-banner="realm-error">
            {realm.error}
          </p>
        ) : null}
        {realm.control === null ? (
          <p className="empty">Attaching to the realm…</p>
        ) : (
          <>
            {view === 'board' ? (
              <BoardView
                control={realm.control}
                selected={selectedRun}
                onSelect={setSelectedRun}
                onOpen={(agentRunId) => void realm.openRun(agentRunId)}
              />
            ) : null}
            {view === 'task' ? (
              <TaskView
                control={realm.control}
                selected={selectedTask}
                onOpen={(projectId, taskId) => {
                  setSelectedTask({ projectId, taskId })
                  void realm.openTask(projectId, taskId)
                }}
              />
            ) : null}
            {view === 'session' ? (
              <SessionView
                client={realm.client}
                realmId={realm.realm?.realm_id ?? null}
                agentRunId={selectedRun}
                onClear={() => setSelectedRun(null)}
              />
            ) : null}
            {view === 'intake' ? <IntakeView /> : null}
            {view === 'workflow' ? <WorkflowView /> : null}
            {view === 'schedule' ? <ScheduleView /> : null}
            {/* Teams reads and writes through the same attached realm client. */}
            {view === 'teams' && realm.client ? <TeamsView client={realm.client} /> : null}
          </>
        )}
      </main>
    </div>
  )
}
