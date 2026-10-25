use super::{id, invalid, text, title};
use crate::*;

fn validate_placement(s: &DesktopSnapshot, space_id: &str, group_id: Option<&str>) -> Result<()> {
    if !s.spaces.iter().any(|space| space.id == space_id)
        || group_id.is_some_and(|group_id| {
            !s.groups
                .iter()
                .any(|g| g.id == group_id && g.space_id == space_id)
        })
    {
        return Err(ServiceError::new(
            "INVALID_PLACEMENT",
            "空间不存在，或分组不属于目标空间",
        ));
    }
    Ok(())
}

impl DesktopService {
    pub fn rename_space(&mut self, revision: u64, space_id: &str, name: String) -> Result<()> {
        let name = title(&name)?;
        self.mutate(|s| {
            check_revision(s, revision)?;
            s.spaces
                .iter_mut()
                .find(|v| v.id == space_id)
                .ok_or_else(|| invalid("空间不存在"))?
                .name = name;
            Ok(())
        })
    }
    pub fn rename_group(&mut self, revision: u64, group_id: &str, name: String) -> Result<()> {
        let name = title(&name)?;
        self.mutate(|s| {
            check_revision(s, revision)?;
            s.groups
                .iter_mut()
                .find(|v| v.id == group_id)
                .ok_or_else(|| invalid("分组不存在"))?
                .name = name;
            Ok(())
        })
    }
    pub fn update_context(
        &mut self,
        revision: u64,
        context_id: &str,
        name: String,
        goal: String,
        resources: Vec<String>,
    ) -> Result<()> {
        let name = title(&name)?;
        text(&goal)?;
        for r in &resources {
            text(r)?;
        }
        self.mutate(|s| {
            check_revision(s, revision)?;
            let c = s
                .contexts
                .iter_mut()
                .find(|c| c.id == context_id)
                .ok_or_else(|| invalid("上下文不存在"))?;
            c.name = name;
            c.goal = goal;
            c.resources = resources;
            Ok(())
        })
    }
    pub fn update_project(
        &mut self,
        revision: u64,
        project_id: &str,
        name: String,
        goal: String,
        resources: Vec<String>,
    ) -> Result<()> {
        let name = title(&name)?;
        text(&goal)?;
        for r in &resources {
            text(r)?;
        }
        self.mutate(|s| {
            check_revision(s, revision)?;
            let p = s
                .projects
                .iter_mut()
                .find(|p| p.id == project_id)
                .ok_or_else(|| invalid("项目不存在"))?;
            p.name = name;
            p.goal = goal;
            p.resources = resources;
            Ok(())
        })
    }
    pub fn bind_conversation_context(
        &mut self,
        conversation_id: &str,
        context_id: Option<String>,
    ) -> Result<()> {
        self.mutate(|s| {
            if context_id
                .as_ref()
                .is_some_and(|id| !s.contexts.iter().any(|c| &c.id == id))
            {
                return Err(invalid("上下文不存在"));
            }
            s.conversations
                .iter_mut()
                .find(|c| c.id == conversation_id)
                .ok_or_else(|| invalid("对话不存在"))?
                .context_id = context_id;
            Ok(())
        })
    }

    pub fn create_space(&mut self, name: String) -> Result<Space> {
        let name = title(&name)?;
        self.mutate(|s| {
            let space = Space {
                id: id("space"),
                name,
            };
            s.spaces.push(space.clone());
            Ok(space)
        })
    }
    pub fn create_group(&mut self, space_id: &str, name: String) -> Result<Group> {
        let name = title(&name)?;
        self.mutate(|s| {
            validate_placement(s, space_id, None)?;
            let group = Group {
                id: id("group"),
                space_id: space_id.into(),
                name,
            };
            s.groups.push(group.clone());
            Ok(group)
        })
    }
    pub fn create_context(
        &mut self,
        name: String,
        goal: String,
        resources: Vec<String>,
        space_id: &str,
        group_id: Option<String>,
    ) -> Result<Context> {
        let name = title(&name)?;
        text(&goal)?;
        for r in &resources {
            text(r)?;
        }
        self.mutate(|s| {
            validate_placement(s, space_id, group_id.as_deref())?;
            let context = Context {
                id: id("context"),
                name,
                goal,
                resources,
            };
            s.placements.push(ContextPlacement {
                context_id: context.id.clone(),
                space_id: space_id.into(),
                group_id,
            });
            s.contexts.push(context.clone());
            Ok(context)
        })
    }
    pub fn move_context(
        &mut self,
        context_id: &str,
        space_id: &str,
        group_id: Option<String>,
    ) -> Result<()> {
        self.mutate(|s| {
            validate_placement(s, space_id, group_id.as_deref())?;
            if !s.contexts.iter().any(|c| c.id == context_id) {
                return Err(invalid("上下文不存在"));
            }
            let location = s
                .placements
                .iter_mut()
                .find(|p| p.context_id == context_id)
                .ok_or_else(|| invalid("上下文位置不存在"))?;
            location.space_id = space_id.into();
            location.group_id = group_id;
            Ok(())
        })
    }
    pub fn bind_project_context(
        &mut self,
        project_id: &str,
        context_id: Option<String>,
    ) -> Result<()> {
        self.mutate(|s| {
            if context_id
                .as_ref()
                .is_some_and(|id| !s.contexts.iter().any(|c| &c.id == id))
            {
                return Err(invalid("上下文不存在"));
            }
            s.projects
                .iter_mut()
                .find(|p| p.id == project_id)
                .ok_or_else(|| invalid("项目不存在"))?
                .context_id = context_id;
            Ok(())
        })
    }
}

pub(super) fn check_revision(s: &DesktopSnapshot, revision: u64) -> Result<()> {
    if s.revision != revision {
        return Err(ServiceError::new(
            "REVISION_CONFLICT",
            "资料已发生变化，请刷新后核对再保存；未覆盖当前数据",
        ));
    }
    Ok(())
}
