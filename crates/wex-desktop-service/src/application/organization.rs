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
