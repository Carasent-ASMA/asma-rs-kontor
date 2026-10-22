//! Domain context for explicit project selection, read from immutable bundles.

use super::*;

pub(super) struct ProjectDomainSelection {
    pub current: Option<ProjectTeamDefinitionDefault>,
    pub target: ProjectTeamDefinitionDefault,
    pub roster_hash: Option<ContentHash>,
    pub preview_hash: ContentHash,
}

impl Services {
    pub(super) fn epic_domain_binding(
        &self,
        project: ProjectId,
        name: &ExternalName,
        expected: Option<&ContentHash>,
    ) -> Result<
        (
            kontor_store::EpicDomainBinding,
            Option<ProjectTeamDefinitionDefault>,
        ),
        ApiError,
    > {
        self.project_row(project)?;
        let state = self.state()?;
        let epic = state
            .with_store(|store| store.list_mini_projects(project))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|epic| &epic.name == name);
        let (definition, bundle, roster, selection) = if let Some(epic) = epic {
            let pin = state
                .with_store(|store| store.get_mini_project_team_definition(project, epic.id))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(ApiErrorCode::InvalidRequest, "operational_domain_unpinned")
                })?;
            let bundle = pin.operational_domain_bundle_hash.ok_or_else(|| {
                self.deny(ApiErrorCode::InvalidRequest, "operational_domain_unpinned")
            })?;
            (
                pin.definition,
                bundle,
                self.frozen_roster(project, epic.id)?.revision,
                None,
            )
        } else {
            let selected = state
                .with_store(|store| store.get_project_team_definition_default(project))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(ApiErrorCode::InvalidRequest, "operational_domain_unpinned")
                })?;
            let bundle = selected
                .operational_domain_bundle_hash
                .clone()
                .ok_or_else(|| {
                    self.deny(ApiErrorCode::InvalidRequest, "operational_domain_unpinned")
                })?;
            let roster = self
                .stored_core_team(project)?
                .ok_or_else(|| self.deny(ApiErrorCode::RoleSlotUnbound, "role_purpose_unpinned"))?;
            (selected.definition.clone(), bundle, roster, Some(selected))
        };
        if expected.is_some_and(|expected| expected != &bundle) {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "operational_domain_epic_target_moved",
            ));
        }
        self.domain_for_definition(project, &definition, Some(&bundle), Some(&roster))?;
        Ok((
            kontor_store::EpicDomainBinding {
                definition,
                operational_domain_bundle_hash: bundle,
                roster,
            },
            selection,
        ))
    }

    /// Replay uses its recorded context, without reading today's selection or YAML.
    pub(super) fn verify_epic_domain_replay(
        &self,
        project: ProjectId,
        epic: MiniProjectId,
        binding: &kontor_store::EpicDomainBinding,
    ) -> Result<(), ApiError> {
        let pin = self
            .state()?
            .with_store(|store| store.get_mini_project_team_definition(project, epic))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(ApiErrorCode::InvalidRequest, "operational_domain_unpinned")
            })?;
        if pin.definition != binding.definition
            || pin.operational_domain_bundle_hash.as_ref()
                != Some(&binding.operational_domain_bundle_hash)
        {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "operational_domain_creation_result_mismatch",
            ));
        }
        // Never reconstruct a missing frozen roster from the current project.
        self.frozen_roster(project, epic)?;
        Ok(())
    }

    fn domain_for_definition(
        &self,
        project: ProjectId,
        definition: &TeamDefinitionSnapshot,
        bundle: Option<&ContentHash>,
        roster: Option<&CoreTeamRevision>,
    ) -> Result<OperationalDomainPack, ApiError> {
        self.project_row(project)?;
        let bundle = bundle.ok_or_else(|| {
            self.deny(ApiErrorCode::InvalidRequest, "operational_domain_unpinned")
        })?;
        let (manifest, domain) = self
            .fleet
            .operational_domain(bundle)
            .map_err(|error| self.refuse_fleet_policy(&error))?;
        let declared = domain
            .team_definitions
            .iter()
            .find(|candidate| {
                candidate.definition_id == definition.definition_id
                    && candidate.version == definition.version
            })
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::InvalidRequest,
                    "operational_domain_definition_mismatch",
                )
            })?;
        if TeamDefinitionSnapshot::from_revision(declared)
            .map_err(|error| self.refuse_domain(&error))?
            != *definition
        {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "operational_domain_definition_mismatch",
            ));
        }
        let state = self.state()?;
        let stored = state
            .with_store(|store| {
                store.get_team_definition(project, definition.definition_id, definition.version)
            })
            .map_err(|error| self.refuse(&error))?;
        if stored.as_ref() != Some(declared) {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "operational_domain_definition_mismatch",
            ));
        }
        let topology = state
            .with_store(|store| {
                store.get_topology_spec(
                    project,
                    declared.topology.spec_id,
                    declared.topology.version,
                )
            })
            .map_err(|error| self.refuse(&error))?;
        if !domain
            .topology_specs
            .iter()
            .any(|candidate| Some(candidate) == topology.as_ref())
        {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "operational_domain_topology_mismatch",
            ));
        }
        // The artifact supplies the exact catalog even before its first seat is
        // published; a stored revision under the same identity must agree.
        if let Some(catalog) = state
            .with_store(|store| {
                store.get_role_catalog(
                    manifest.role_catalog.catalog_id,
                    manifest.role_catalog.version,
                )
            })
            .map_err(|error| self.refuse(&error))?
            && catalog
                .canonicalize()
                .map_err(|error| self.refuse_domain(&error))?
                .hash()
                != &manifest.role_catalog.content_hash
        {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "operational_domain_catalog_mismatch",
            ));
        }
        if let Some(roster) = roster {
            kontor_fleet_activation::domain::validate_roster(
                &domain,
                &manifest.role_catalog,
                roster,
            )
            .map_err(|error| self.refuse_fleet_policy(&error))?;
        }
        Ok(domain)
    }

    pub(super) fn project_team_definition_selection(
        &self,
        project_id: ProjectId,
        target: &TeamDefinitionRefDto,
    ) -> Result<ProjectDomainSelection, ApiError> {
        let state = self.state()?;
        self.project_row(project_id)?;
        let current = state
            .with_store(|store| store.get_project_team_definition_default(project_id))
            .map_err(|error| self.refuse(&error))?;
        let definition = state
            .with_store(|store| store.get_team_definition(project_id, target.id, target.version))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the target Team Definition is not published in this project",
                )
            })?;
        let definition = TeamDefinitionSnapshot::from_revision(&definition)
            .map_err(|error| self.refuse_domain(&error))?;
        let bundle = self
            .fleet
            .active_domain_bundle()
            .map_err(|error| self.refuse_fleet_policy(&error))?;
        let roster = self.stored_core_team(project_id)?;
        self.domain_for_definition(project_id, &definition, Some(&bundle), roster.as_ref())?;
        if current.as_ref().is_some_and(|held| {
            held.definition == definition
                && held.operational_domain_bundle_hash.as_ref() == Some(&bundle)
        }) {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "the project already selects that Team Definition and operational domain",
            ));
        }
        let roster_hash = roster
            .as_ref()
            .map(CoreTeamRevision::canonicalize)
            .transpose()
            .map_err(|error| self.refuse_domain(&error))?
            .map(|document| document.hash().clone());
        let preview_hash = self.preview_hash(&serde_json::json!({
            "schema_version": 1,
            "operation": "project_team_definition_selection_preview",
            "project": project_id,
            "current": current.as_ref().map(|held| serde_json::json!({
                "definition": held.definition,
                "operational_domain_bundle_hash": held.operational_domain_bundle_hash,
            })),
            "target": {"definition": definition, "operational_domain_bundle_hash": bundle},
            "core_team_revision_hash": roster_hash,
        }))?;
        Ok(ProjectDomainSelection {
            target: ProjectTeamDefinitionDefault {
                project_id,
                expected: current.as_ref().map(|held| held.definition.clone()),
                definition,
                operational_domain_bundle_hash: Some(bundle),
                selected_at: kontor_api::now(),
            },
            current,
            roster_hash,
            preview_hash,
        })
    }

    pub(super) fn target_of_project_team_definition_preview(
        &self,
        project_id: ProjectId,
        preview_hash: &ContentHash,
    ) -> Result<ProjectDomainSelection, ApiError> {
        for candidate in self
            .state()?
            .with_store(|store| store.list_team_definitions(project_id))
            .map_err(|error| self.refuse(&error))?
        {
            let reference = TeamDefinitionRefDto {
                id: candidate.definition_id,
                version: candidate.version,
            };
            let Ok(selection) = self.project_team_definition_selection(project_id, &reference)
            else {
                continue;
            };
            if &selection.preview_hash == preview_hash {
                return Ok(selection);
            }
        }
        Err(self.deny(
            ApiErrorCode::RevisionConflict,
            "operational_domain_selection_preview_moved",
        ))
    }
}
