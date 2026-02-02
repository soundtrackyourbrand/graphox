import { gql } from 'graphql-tag';

const GET_USERS = gql`
  query GetUsers {
    users {
      id
      username
    }
  }
`;

const GET_POSTS = gql`
  query GetPosts {
    posts {
      id
      title
      author {
        username
      }
    }
  }
`;

// A larger component with multiple queries
function MyComponent() {
  const query = gql`
    query LocalQuery {
      node(id: "123") {
        ... on User {
          username
        }
      }
    }
  `;
}
