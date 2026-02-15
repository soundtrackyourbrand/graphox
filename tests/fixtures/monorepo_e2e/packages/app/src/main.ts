import { graphql } from './__generated__';

// Inline query with variables, directives, and aliases
const GetUsersWithComplexFilters = graphql(`
  query GetUsersWithComplexFilters(
    $first: Int = 10
    $after: String
    $role: UserRole
    $skipEmails: Boolean!
    $includeStats: Boolean!
  ) {
    users(first: $first, after: $after, role: $role) {
      nodes {
        id
        name
        email @skip(if: $skipEmails)
        role
        totalPosts @include(if: $includeStats)
        totalComments @include(if: $includeStats)
        posts(first: 3) {
          nodes {
            id
            title
            status
            viewCount
            commentCount
            tags
          }
        }
      }
      pageInfo {
        hasNextPage
        endCursor
      }
      totalCount
    }
  }
`);

// Query with union type
const SearchEverything = graphql(`
  query SearchEverything($query: String!, $includeUsers: Boolean = true, $includePosts: Boolean = true) {
    search(input: { query: $query }) {
      ... on User @include(if: $includeUsers) {
        id
        name
        email
        role
      }
      ... on Post @include(if: $includePosts) {
        id
        title
        postStatus: status
        viewCount
        author {
          id
          name
        }
      }
      ... on Comment @include(if: $includePosts) {
        id
        body
        commentStatus: status
        author {
          id
          name
        }
      }
    }
  }
`);

// Mutation with input type
const CreateNewUser = graphql(`
  mutation CreateNewUser($input: CreateUserInput!) {
    createUser(input: $input) {
      user {
        id
        name
        email
        role
        createdAt
      }
      errors {
        message
        code
      }
    }
  }
`);

// Export queries
export { GetUsersWithComplexFilters, SearchEverything, CreateNewUser };
