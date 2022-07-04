use crate::dart_notification::{send_dart_notification, GridNotification};
use crate::entities::{
    FieldType, GridBlockChangeset, GridCheckboxFilter, GridDateFilter, GridNumberFilter, GridRowId,
    GridSelectOptionFilter, GridTextFilter, InsertedRow,
};
use crate::services::block_manager::GridBlockManager;
use crate::services::grid_editor_task::GridServiceTaskScheduler;
use crate::services::row::GridBlockSnapshot;
use crate::services::tasks::{FilterTaskContext, Task, TaskContent};
use flowy_error::FlowyResult;
use flowy_grid_data_model::revision::{CellRevision, FieldId, FieldRevision, RowRevision};
use flowy_sync::client_grid::GridRevisionPad;
use flowy_sync::entities::grid::GridSettingChangesetParams;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub(crate) struct GridFilterService {
    #[allow(dead_code)]
    grid_id: String,
    scheduler: Arc<dyn GridServiceTaskScheduler>,
    grid_pad: Arc<RwLock<GridRevisionPad>>,
    block_manager: Arc<GridBlockManager>,
    filter_cache: Arc<RwLock<FilterCache>>,
    filter_result_cache: Arc<RwLock<FilterResultCache>>,
}
impl GridFilterService {
    pub async fn new<S: GridServiceTaskScheduler>(
        grid_pad: Arc<RwLock<GridRevisionPad>>,
        block_manager: Arc<GridBlockManager>,
        scheduler: S,
    ) -> Self {
        let grid_id = grid_pad.read().await.grid_id();
        let filter_cache = Arc::new(RwLock::new(FilterCache::from_grid_pad(&grid_pad).await));
        let filter_result_cache = Arc::new(RwLock::new(FilterResultCache::default()));
        Self {
            grid_id,
            grid_pad,
            block_manager,
            scheduler: Arc::new(scheduler),
            filter_cache,
            filter_result_cache,
        }
    }

    pub async fn process(&self, task_context: FilterTaskContext) -> FlowyResult<()> {
        let field_revs = self
            .grid_pad
            .read()
            .await
            .get_field_revs(None)?
            .into_iter()
            .map(|field_rev| (field_rev.id.clone(), field_rev))
            .collect::<HashMap<String, Arc<FieldRevision>>>();

        let mut changesets = vec![];
        for (index, block) in task_context.blocks.into_iter().enumerate() {
            let mut inserted_rows = vec![];
            let mut deleted_rows = vec![];
            block.row_revs.iter().for_each(|row_rev| {
                let result = filter_row(
                    index,
                    row_rev,
                    &self.filter_cache,
                    &self.filter_result_cache,
                    &field_revs,
                );
                if result.is_visible() {
                    inserted_rows.push(InsertedRow {
                        row_id: Default::default(),
                        block_id: Default::default(),
                        height: 1,
                        index: Some(result.row_index),
                    });
                } else {
                    deleted_rows.push(GridRowId {
                        grid_id: self.grid_id.clone(),
                        block_id: block.block_id.clone(),
                        row_id: result.row_id,
                    });
                }
            });

            let changeset = GridBlockChangeset {
                block_id: block.block_id,
                inserted_rows,
                deleted_rows,
                updated_rows: vec![],
            };
            changesets.push(changeset);
        }
        self.notify(changesets).await;
        Ok(())
    }

    pub async fn apply_changeset(&self, changeset: GridFilterChangeset) {
        if !changeset.is_changed() {
            return;
        }

        if let Some(filter_id) = &changeset.insert_filter {
            let mut cache = self.filter_cache.write().await;
            let field_ids = Some(vec![filter_id.field_id.clone()]);
            reload_filter_cache(&mut cache, field_ids, &self.grid_pad).await;
        }

        if let Some(filter_id) = &changeset.delete_filter {
            self.filter_cache.write().await.remove(filter_id);
        }

        if let Ok(blocks) = self.block_manager.get_block_snapshots(None).await {
            let task = self.gen_task(blocks).await;
            let _ = self.scheduler.register_task(task).await;
        }
    }

    async fn gen_task(&self, blocks: Vec<GridBlockSnapshot>) -> Task {
        let task_id = self.scheduler.gen_task_id().await;
        let handler_id = self.grid_pad.read().await.grid_id();

        let context = FilterTaskContext { blocks };
        Task {
            handler_id,
            id: task_id,
            content: TaskContent::Filter(context),
        }
    }

    async fn notify(&self, changesets: Vec<GridBlockChangeset>) {
        for changeset in changesets {
            send_dart_notification(&self.grid_id, GridNotification::DidUpdateGridBlock)
                .payload(changeset)
                .send();
        }
    }
}

fn filter_row(
    index: usize,
    row_rev: &Arc<RowRevision>,
    _filter_cache: &Arc<RwLock<FilterCache>>,
    _filter_result_cache: &Arc<RwLock<FilterResultCache>>,
    _field_revs: &HashMap<FieldId, Arc<FieldRevision>>,
) -> FilterResult {
    let filter_result = FilterResult::new(index as i32, row_rev);
    row_rev.cells.iter().for_each(|(_k, cell_rev)| {
        let _cell_rev: &CellRevision = cell_rev;
    });
    filter_result
}

pub struct GridFilterChangeset {
    insert_filter: Option<FilterId>,
    delete_filter: Option<FilterId>,
}

impl GridFilterChangeset {
    fn is_changed(&self) -> bool {
        self.insert_filter.is_some() || self.delete_filter.is_some()
    }
}

impl std::convert::From<&GridSettingChangesetParams> for GridFilterChangeset {
    fn from(params: &GridSettingChangesetParams) -> Self {
        let insert_filter = params.insert_filter.as_ref().map(|insert_filter_params| FilterId {
            field_id: insert_filter_params.field_id.clone(),
            field_type: insert_filter_params.field_type_rev.into(),
        });

        let delete_filter = params.delete_filter.as_ref().map(|delete_filter_params| FilterId {
            field_id: delete_filter_params.filter_id.clone(),
            field_type: delete_filter_params.field_type_rev.into(),
        });
        GridFilterChangeset {
            insert_filter,
            delete_filter,
        }
    }
}

#[derive(Default)]
struct FilterResultCache {
    #[allow(dead_code)]
    rows: HashMap<String, FilterResult>,
}

impl FilterResultCache {
    #[allow(dead_code)]
    fn insert(&mut self, row_id: &str, result: FilterResult) {
        self.rows.insert(row_id.to_owned(), result);
    }
}

#[derive(Default)]
struct FilterResult {
    row_id: String,
    row_index: i32,
    cell_by_field_id: HashMap<String, bool>,
}

impl FilterResult {
    fn new(index: i32, row_rev: &RowRevision) -> Self {
        Self {
            row_index: index,
            row_id: row_rev.id.clone(),
            cell_by_field_id: row_rev.cells.iter().map(|(k, _)| (k.clone(), true)).collect(),
        }
    }

    #[allow(dead_code)]
    fn update_cell(&mut self, cell_id: &str, exist: bool) {
        self.cell_by_field_id.insert(cell_id.to_owned(), exist);
    }

    fn is_visible(&self) -> bool {
        todo!()
    }
}

#[derive(Default)]
struct FilterCache {
    text_filter: HashMap<FilterId, GridTextFilter>,
    url_filter: HashMap<FilterId, GridTextFilter>,
    number_filter: HashMap<FilterId, GridNumberFilter>,
    date_filter: HashMap<FilterId, GridDateFilter>,
    select_option_filter: HashMap<FilterId, GridSelectOptionFilter>,
    checkbox_filter: HashMap<FilterId, GridCheckboxFilter>,
}

impl FilterCache {
    async fn from_grid_pad(grid_pad: &Arc<RwLock<GridRevisionPad>>) -> Self {
        let mut this = Self::default();
        let _ = reload_filter_cache(&mut this, None, grid_pad).await;
        this
    }

    fn remove(&mut self, filter_id: &FilterId) {
        let _ = match filter_id.field_type {
            FieldType::RichText => {
                let _ = self.text_filter.remove(filter_id);
            }
            FieldType::Number => {
                let _ = self.number_filter.remove(filter_id);
            }
            FieldType::DateTime => {
                let _ = self.date_filter.remove(filter_id);
            }
            FieldType::SingleSelect => {
                let _ = self.select_option_filter.remove(filter_id);
            }
            FieldType::MultiSelect => {
                let _ = self.select_option_filter.remove(filter_id);
            }
            FieldType::Checkbox => {
                let _ = self.checkbox_filter.remove(filter_id);
            }
            FieldType::URL => {
                let _ = self.url_filter.remove(filter_id);
            }
        };
    }
}

async fn reload_filter_cache(
    cache: &mut FilterCache,
    field_ids: Option<Vec<String>>,
    grid_pad: &Arc<RwLock<GridRevisionPad>>,
) {
    let grid_pad = grid_pad.read().await;
    let filters_revs = grid_pad.get_filters(None, field_ids).unwrap_or_default();

    for filter_rev in filters_revs {
        match grid_pad.get_field_rev(&filter_rev.field_id) {
            None => {}
            Some((_, field_rev)) => {
                let filter_id = FilterId::from(field_rev);
                let field_type: FieldType = field_rev.field_type_rev.into();
                match &field_type {
                    FieldType::RichText => {
                        let _ = cache.text_filter.insert(filter_id, GridTextFilter::from(filter_rev));
                    }
                    FieldType::Number => {
                        let _ = cache
                            .number_filter
                            .insert(filter_id, GridNumberFilter::from(filter_rev));
                    }
                    FieldType::DateTime => {
                        let _ = cache.date_filter.insert(filter_id, GridDateFilter::from(filter_rev));
                    }
                    FieldType::SingleSelect | FieldType::MultiSelect => {
                        let _ = cache
                            .select_option_filter
                            .insert(filter_id, GridSelectOptionFilter::from(filter_rev));
                    }
                    FieldType::Checkbox => {
                        let _ = cache
                            .checkbox_filter
                            .insert(filter_id, GridCheckboxFilter::from(filter_rev));
                    }
                    FieldType::URL => {
                        let _ = cache.url_filter.insert(filter_id, GridTextFilter::from(filter_rev));
                    }
                }
            }
        }
    }
}
#[derive(Hash, Eq, PartialEq)]
struct FilterId {
    field_id: String,
    field_type: FieldType,
}

impl std::convert::From<&Arc<FieldRevision>> for FilterId {
    fn from(rev: &Arc<FieldRevision>) -> Self {
        Self {
            field_id: rev.id.clone(),
            field_type: rev.field_type_rev.into(),
        }
    }
}
