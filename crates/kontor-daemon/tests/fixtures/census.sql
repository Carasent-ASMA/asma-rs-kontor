-- Read-only coherent census, including pending and tombstoned items.
-- Run inside one read transaction on an approved isolated copy. SQL shape is
-- descriptive; typed eligibility still comes from classify_experiences.
SELECT i.project_id, i.id, i.aggregate_revision, i.current_revision_id,
       r.id AS latest_revision_id, r.revision, r.content_hash,
       json_extract(r.provenance,'$.source') AS provenance,
       CASE WHEN t.item_id IS NOT NULL THEN 'tombstoned'
            WHEN i.current_revision_id IS NULL THEN 'pending'
            WHEN json_extract(c.document,'$.document_type')='experience_memory'
              THEN 'typed_candidate_requires_rust_validation'
            ELSE 'generic' END AS classification,
       json_extract(c.document,'$.projection_policy') AS policy
FROM memory_items i
LEFT JOIN memory_revisions r ON r.project_id=i.project_id AND r.item_id=i.id
 AND r.revision=(SELECT MAX(h.revision) FROM memory_revisions h
                 WHERE h.project_id=i.project_id AND h.item_id=i.id)
LEFT JOIN memory_revisions c ON c.project_id=i.project_id AND c.id=i.current_revision_id
LEFT JOIN memory_tombstones t ON t.project_id=i.project_id AND t.item_id=i.id
ORDER BY i.project_id,i.id;
