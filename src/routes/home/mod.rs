use axum::{response::Html, Extension};

use crate::domain::AppUser;

pub async fn home(Extension(user): Extension<AppUser>) -> Html<String> {
    let user_name = user.name;
    Html(format!(
        r#"<!DOCTYPE html>
<html lang="en">
    <head>
        <meta http-equiv="content-type" content="text/html; charset=utf-8">
        <title>Home</title>
    </head>
    <body>
        <p>Welcome {}!</p>
        <p>Actions:</p>
        <ol>
            <li><a href="/content/form">Post Content</a></li>
            <li><a href="/user/change-password">Change your password</a></li>
            <li>
                <form name="logout_form" action="/user/logout" method="post">
                    <input type="submit" value="Logout" />
                </form>
            </li>
        </ol>
    </body>
</html>"#,
        user_name
    ))
}
