/**
 * The intake inbox: what arrived, what matched it, and what it became.
 *
 * The lineage column is the point. An intake item that created work says which
 * work; one that did not says so. Without it an operator cannot tell an item
 * that was approved and acted on from one that was approved and dropped.
 */
import type { IntakeItem } from '../views/projections'
import { StateBadge } from './primitives'

/** Render the intake inbox. */
export function IntakeInbox({
  items,
  onSelect,
  selectedId,
}: {
  /** The receipts, newest first as the realm ordered them. */
  items: readonly IntakeItem[]
  /** Called when an operator picks one. */
  onSelect?: (receiptId: string) => void
  /** The receipt currently picked. */
  selectedId?: string | null
}) {
  if (items.length === 0) {
    return <p className="empty">no intake has been received</p>
  }
  return (
    <table className="intake">
      <caption>intake receipts</caption>
      <thead>
        <tr>
          <th scope="col">source</th>
          <th scope="col">dedup key</th>
          <th scope="col">matched trigger</th>
          <th scope="col">approval</th>
          <th scope="col">created work</th>
          <th scope="col">received</th>
        </tr>
      </thead>
      <tbody>
        {items.map((item) => (
          <tr
            key={item.receiptId}
            data-receipt-id={item.receiptId}
            aria-selected={selectedId === item.receiptId}
            onClick={onSelect ? () => onSelect(item.receiptId) : undefined}
          >
            <th scope="row">{item.source}</th>
            <td>
              <code>{item.dedupKey}</code>
            </td>
            <td>
              {item.triggerId === null ? (
                <span className="empty">no trigger matched</span>
              ) : (
                <code>
                  {item.triggerId}
                  {item.triggerVersion === null ? '' : ` @${item.triggerVersion}`}
                </code>
              )}
            </td>
            <td>
              <StateBadge state={item.approvalState} label="approval" />
            </td>
            <td>
              {item.createdWork.length === 0 ? (
                <span className="empty">none</span>
              ) : (
                <ul className="lineage">
                  {item.createdWork.map((created) => (
                    <li key={`${created.kind}:${created.id}`}>
                      {created.kind} <code>{created.id}</code>
                    </li>
                  ))}
                </ul>
              )}
            </td>
            <td>
              <time dateTime={item.receivedAt}>{item.receivedAt}</time>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}
