use axum::{extract::{Extension, Query}, Json};
use futures::TryStreamExt;

use crate::http::{extractor::{MaybeAuthUser, AuthUser}, ApiContext, types::Timestamptz};

use super::{Article, ArticleFromQuery, Result};

#[derive(serde::Deserialize, Default)]
#[serde(default)]
pub struct ListArticleQuery {
    tag: Option<String>,
    author: Option<String>,
    favorited: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
pub struct FeedArticlesQuery {
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultipleArticlesBody {
    articles: Vec<Article>,
    articles_count: usize,
}

pub(in crate::http) async fn list_articles(
    maybe_auth_user: MaybeAuthUser,
    ctx: Extension<ApiContext>,
    query: Query<ListArticleQuery>,
) -> Result<Json<MultipleArticlesBody>> {
    let articles: Vec<_> = sqlx::query_as!(
        ArticleFromQuery,
        r#"
select
    slug,
    title,
    description,
    body,
    tag_list,
    article.created_at `created_at: Timestamptz`,
    article.updated_at `updated_at: Timestamptz`,
    exists(select 1 from article_favorite where user_id = ?) `favorited!:_`,
    coalesce(
        (select count(*) from article_favorite fav where fav.article_id = article.article_id),
        0
    ) `favorites_count!`,
    author.username author_username,
    author.bio author_bio,
    author.image author_image,
    exists(select 1 from follow where followed_user_id = author.user_id and following_user_id = ?) `following_author!:_`
from article
inner join user author using (user_id)
where (
    ? is null or author.username = ?
) and (
    ? is null or JSON_CONTAINS(article.tag_list, JSON_ARRAY(?))
) and (
    ? is null or exists(
        select 1 from user
        inner join article_favorite af using (user_id)
        where user.username = ?
        and af.article_id = article.article_id
    )
)
order by article.created_at desc
limit ?
offset ?
    "#,
    maybe_auth_user.user_id().map(|id| id.to_string()),
    maybe_auth_user.user_id().map(|id| id.to_string()),
    query.author,
    query.author,
    query.tag,
    query.tag,
    query.favorited,
    query.favorited,
    query.limit.unwrap_or(20),
    query.offset.unwrap_or(0)
    )
        .fetch(&ctx.db)
        .map_ok(ArticleFromQuery::into_article)
        .try_collect()
        .await?;

    Ok(Json(MultipleArticlesBody {
        articles_count: articles.len(),
        articles,
    }))
}

pub(in crate::http) async fn feed_articles(
    auth_user: AuthUser,
    ctx: Extension<ApiContext>,
    query: Query<FeedArticlesQuery>,
) -> Result<Json<MultipleArticlesBody>> {
    let articles: Vec<_> = sqlx::query_as!(
        ArticleFromQuery,
        r#"
select
    slug,
    title,
    description,
    body,
    tag_list,
    article.created_at `created_at: Timestamptz`,
    article.updated_at `updated_at: Timestamptz`,
    exists(select 1 from article_favorite where user_id = ?) `favorited!:_`,
    coalesce(
        (select count(*) from article_favorite fav where fav.article_id = article.article_id),
        0
    ) `favorites_count!`,
    author.username author_username,
    author.bio author_bio,
    author.image author_image,
    1 `following_author!:_`
from follow
inner join article on followed_user_id = article.user_id
inner join user author using (user_id)
where following_user_id = ?
order by article.created_at desc
limit ?
offset ?
        "#,
        auth_user.user_id.to_string(),
        auth_user.user_id.to_string(),
        query.limit.unwrap_or(20),
        query.offset.unwrap_or(0)
    )
        .fetch(&ctx.db)
        .map_ok(ArticleFromQuery::into_article)
        .try_collect()
        .await?;

    Ok(Json(MultipleArticlesBody {
        articles_count: articles.len(),
        articles,
    }))
}