use crate::support::{create_doc, pos_for_token, with_cursors};
use graphox::features::selection_range::DocumentSelectionRange;
use tower_lsp::lsp_types::*;

fn count_parent_chain(range: &SelectionRange) -> usize {
    let mut count = 0;
    let mut current = range;
    while let Some(ref parent) = current.parent {
        count += 1;
        current = parent;
    }
    count
}

#[test]
fn test_selection_range_field() {
    let text = r#"query GetUser {
  user {
    id
    |name
  }
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(ranges.len(), 1, "Should return one selection range");

    let depth = count_parent_chain(&ranges[0]);
    assert!(
        depth >= 3,
        "Should have at least 3 parent levels (field -> selection_set -> operation -> ...)"
    );
}

#[test]
fn test_selection_range_multiple_positions() {
    let text = r#"query GetUser {
  user {
    |id
    |name
    |email
  }
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        3,
        "Should return selection ranges for all positions"
    );

    for range in &ranges {
        assert!(
            range.parent.is_some(),
            "Each selection range should have parent ranges"
        );
    }
}

#[test]
fn test_selection_range_argument() {
    let text = r#"query GetUser($id: ID!) {
  user(id: |$id) {
    name
  }
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for argument"
    );

    // Should have multiple parent levels
    let depth = count_parent_chain(&ranges[0]);
    assert!(depth >= 2, "Argument should have parent ranges");
}

#[test]
fn test_selection_range_fragment() {
    let text = r#"fragment UserFields on User {
  id
  |name
  email
  profile {
    bio
    avatar
  }
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(ranges.len(), 1);

    let depth = count_parent_chain(&ranges[0]);
    assert!(
        depth >= 3,
        "Fragment field should have multiple parent ranges"
    );
}

#[test]
fn test_selection_range_nested_selection_sets() {
    let text = r#"query GetUserWithPosts {
  user {
    id
    name
    posts {
      id
      title
      comments {
        id
        text
        author {
          |name
        }
      }
    }
  }
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(ranges.len(), 1);

    // Should have many parent levels due to deep nesting
    let depth = count_parent_chain(&ranges[0]);
    assert!(
        depth >= 6,
        "Deeply nested field should have many parent ranges"
    );
}

#[test]
fn test_selection_range_inline_fragment() {
    let text = r#"query GetNode {
  node {
    ... on User {
      id
      |name
    }
    ... on Post {
      id
      title
    }
  }
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(ranges.len(), 1);

    let depth = count_parent_chain(&ranges[0]);
    assert!(
        depth >= 4,
        "Field in inline fragment should have parent ranges"
    );
}

#[test]
fn test_selection_range_variable_definitions() {
    let text = r#"query GetUser(
  |$id: ID!
  $includeEmail: Boolean!
) {
  user(id: $id) {
    id
    name
  }
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for variable definition"
    );

    let depth = count_parent_chain(&ranges[0]);
    assert!(depth >= 2, "Variable definition should have parent ranges");
}

#[test]
fn test_selection_range_directive() {
    let text = r#"query GetUser($includeEmail: Boolean!) {
  user {
    id
    name
    email @|include(if: $includeEmail)
  }
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for directive"
    );
}

#[test]
fn test_selection_range_schema_type() {
    let text = r#"type User {
  id: ID!
  |name: String!
  email: String!
  posts: [Post!]!
}

type Post {
  id: ID!
  title: String!
  content: String!
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for schema field"
    );

    let depth = count_parent_chain(&ranges[0]);
    assert!(depth >= 2, "Schema field should have parent ranges");
}

#[test]
fn test_selection_range_enum() {
    let text = r#"enum Role {
  ADMIN
  |USER
  GUEST
  MODERATOR
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for enum value"
    );

    let depth = count_parent_chain(&ranges[0]);
    assert!(depth >= 1, "Enum value should have parent ranges");
}

#[test]
fn test_selection_range_input_type() {
    let text = r#"input CreateUserInput {
  name: String!
  |email: String!
  role: Role!
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for input field"
    );

    let depth = count_parent_chain(&ranges[0]);
    assert!(depth >= 2, "Input field should have parent ranges");
}

#[test]
fn test_selection_range_tsx_embedded() {
    let text = r#"
const query = gql`
  query GetUser($id: ID!) {
    user(id: $id) {
      id
      |name
      email
    }
  }
`;
"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.tsx", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for embedded GraphQL in TSX"
    );

    let depth = count_parent_chain(&ranges[0]);
    assert!(
        depth >= 3,
        "Embedded GraphQL should have proper parent ranges"
    );
}

#[test]
fn test_selection_range_empty_query() {
    let text = "query EmptyQuery {\n|}";
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    // May or may not return a range depending on exact cursor position
    // This tests that we handle edge cases gracefully
    assert!(ranges.len() <= 1, "Should handle empty query gracefully");
}

#[test]
fn test_selection_range_alias() {
    let text = r#"query GetUser {
  user {
    |userId: id
    userName: name
    userEmail: email
  }
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for aliased field"
    );
}

#[test]
fn test_selection_range_extends() {
    let text = r#"type User {
  id: ID!
  name: String!
}

extend type User {
  |email: String!
  posts: [Post!]!
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for field in type extension"
    );

    let depth = count_parent_chain(&ranges[0]);
    assert!(
        depth >= 2,
        "Field in type extension should have parent ranges"
    );
}

#[test]
fn test_selection_range_interface() {
    let text = r#"interface Node {
  id: ID!
}

type User implements Node {
  |id: ID!
  name: String!
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for interface field"
    );
}

#[test]
fn test_selection_range_union() {
    let text = r#"union SearchResult = User | Post | Comment

type User {
  id: ID!
  name: String!
}
"#;
    let doc = create_doc("file:///test.graphql", text);
    let position = pos_for_token(&doc, text, "SearchResult");

    let ranges = doc.get_selection_ranges(vec![position]);
    // Union definitions may or may not provide selection ranges depending on cursor position
    assert!(
        ranges.len() <= 1,
        "Should handle union definition gracefully"
    );
}

#[test]
fn test_selection_range_mutation() {
    let text = r#"mutation CreateUser($input: CreateUserInput!) {
  createUser(input: $input) {
    id
    |name
    email
  }
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for mutation field"
    );

    let depth = count_parent_chain(&ranges[0]);
    assert!(depth >= 3, "Mutation field should have parent ranges");
}

#[test]
fn test_selection_range_subscription() {
    let text = r#"subscription OnMessageAdded($roomId: ID!) {
  messageAdded(roomId: $roomId) {
    id
    |text
    author {
      name
    }
  }
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for subscription field"
    );

    let depth = count_parent_chain(&ranges[0]);
    assert!(depth >= 3, "Subscription field should have parent ranges");
}

#[test]
fn test_selection_range_list_type() {
    let text = r#"type User {
  |posts: [Post!]!
  tags: [String]
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for list type field"
    );
}

#[test]
fn test_selection_range_object_argument() {
    let text = r#"query ComplexQuery {
  search(filter: {
    |status: ACTIVE
    category: "tech"
    tags: ["graphql", "rust"]
  }) {
    id
  }
}"#;
    let (text, positions) = with_cursors(text);
    let doc = create_doc("file:///test.graphql", &text);

    let ranges = doc.get_selection_ranges(positions);
    assert_eq!(
        ranges.len(),
        1,
        "Should return selection range for object argument field"
    );

    let depth = count_parent_chain(&ranges[0]);
    assert!(
        depth >= 3,
        "Object argument field should have parent ranges"
    );
}
