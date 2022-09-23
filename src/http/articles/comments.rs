use axum::{Router, extract::{Extension, Path}, Json, routing::{get, delete}};
use futures::TryStreamExt;
use time::OffsetDateTime;

use crate::http::{types::{Timestamptz, DbBool}, profiles::Profile, extractor::{MaybeAuthUser, AuthUser}, ApiContext, Result, Error};

pub fn router() -> Router {
    Router::new()
        .route(
            "/api/articles/:slug/comments", 
            get(get_article_comments).post(add_comment),
        )
        .route(
            "/api/articles/:slug/comments/:comment_id",
            delete(delete_comment),
        )
}

#[derive(serde::Deserialize, serde::Serialize)]
struct CommentBody<T = Comment> {
    comment: T,
}

#[derive(serde::Serialize)]
struct MultipleCommentsBody {
    comments: Vec<Comment>,
}

#[derive(serde::Deserialize)]
struct AddComment {
    body: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Comment {
    id: i64,
    created_at: Timestamptz,
    updated_at: Timestamptz,
    body: String,
    author: Profile,
}

struct CommentFromQuery {
    comment_id: i64,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    body: String,
    author_username: String,
    author_bio: String,
    author_image: Option<String>,
    following_author: DbBool,
}

impl CommentFromQuery {
    fn into_comment(self) -> Comment {
        Comment {
            id: self.comment_id,
            created_at: Timestamptz(self.created_at),
            updated_at: Timestamptz(self.updated_at),
            body: self.body,
            author: Profile {
                username: self.author_username,
                bio: self.author_bio,
                image: self.author_image,
                following: self.following_author,
            },
        }
    }
}

async fn get_article_comments(
    maybe_auth_user: MaybeAuthUser,
    ctx: Extension<ApiContext>,
    Path(slug): Path<String>,
) -> Result<Json<MultipleCommentsBody>> {
    let article_id = sqlx::query_scalar!(
        r#"
select article_id from article where slug = ?
        "#,
        slug
    )
        .fetch_optional(&ctx.db)
        .await?
        .ok_or(Error::NotFound)?;

    let comments: Vec<_> = sqlx::query_as!(
        CommentFromQuery,
        r#"
select
    comment.comment_id,
    comment.created_at,
    comment.updated_at,
    comment.body,
    author.username author_username,
    author.bio author_bio,
    author.image author_image,
    exists(
        select 1 from follow where followed_user_id = author.user_id and following_user_id = ?
    ) `following_author!:_`
from article_comment comment
inner join user author on author.user_id = comment.user_id
where article_id = ?
order by created_at
        "#,
        maybe_auth_user.user_id().map(|id| id.to_string()),
        article_id
    )
        .fetch(&ctx.db)
        .map_ok(CommentFromQuery::into_comment)
        .try_collect()
        .await?;

    Ok(Json(MultipleCommentsBody { comments }))
}

async fn add_comment(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    Path(slug): Path<String>,
    req: Json<CommentBody<AddComment>>,
) -> Result<Json<CommentBody>> {
    let mut tx = ctx.db.begin().await?;

    let article_id = sqlx::query_scalar!(
        r#"
select article_id from article where slug = ?
        "#,
        slug
    )
        .fetch_optional(&mut tx)
        .await?
        .ok_or(Error::NotFound)?;

    let insert_comment_result = sqlx::query_scalar!(
        r#"
insert into article_comment(article_id, user_id, body) values (?, ?, ?)
        "#,
        article_id,
        auth_user.user_id.to_string(),
        req.comment.body
    )
        .execute(&mut tx)
        .await?;

    let comment = sqlx::query_as!(
        CommentFromQuery,
        r#"
select
    comment.comment_id,
    comment.created_at,
    comment.updated_at,
    comment.body,
    author.username author_username,
    author.bio author_bio,
    author.image author_image,
    0 `following_author!:_`
    from article_comment comment
    inner join user author on author.user_id = comment.user_id
    where comment.comment_id = ?
        "#,
        insert_comment_result.last_insert_id()
    )
        .fetch_one(&mut tx)
        .await?
        .into_comment();

    tx.commit().await?;

    Ok(Json(CommentBody { comment }))
}

async fn delete_comment(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    Path((slug, comment_id)): Path<(String, i64)>,
) -> Result<()> {
    let mut tx = ctx.db.begin().await?;

    let exists = sqlx::query_scalar!(
        r#"
select exists(
    select 1 from article_comment comment
    inner join article on article.article_id = comment.article_id
    where comment.comment_id = ? and article.slug = ?
) "!:_"
        "#,
        comment_id,
        slug
    )
        .fetch_one(&mut tx)
        .await?;
    
    if exists == 0 {
        return Err(Error::NotFound);
    }

    let delete_comment_result = sqlx::query_scalar!(
        r#"
delete from article_comment
    where
        comment_id = ?
        and article_id in (select article_id from article where slug = ?)
        and user_id = ?
        "#,
        comment_id,
        slug,
        auth_user.user_id.to_string()
    )
        .execute(&mut tx)
        .await?;

    tx.commit().await?;

    if delete_comment_result.rows_affected() == 0 {
        return Err(Error::Forbidden);
    }

    Ok(())
}