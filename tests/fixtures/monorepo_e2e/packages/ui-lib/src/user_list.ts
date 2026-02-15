import { graphql } from './__generated__/graphql';

// Query with complex filtering and directives
const GetUsersWithFilters = graphql(`
  query GetUsersWithFilters(
    $userFilter: JSON
    $first: Int
    $after: String
    $skipEmails: Boolean!
    $includeStats: Boolean!
  ) {
    users(filter: $userFilter, first: $first, after: $after) {
      nodes {
        id
        name
        userName: name
        email @skip(if: $skipEmails)
        role
        metadata
        createdAt
        totalPosts @include(if: $includeStats)
        totalComments @include(if: $includeStats)
        posts(first: 5) {
          nodes {
            id
            title
            status
            viewCount
            tags
          }
        }
      }
    }
  }
`);

// Query with union type (SearchResult)
const SearchUsersAndPostsInLib = graphql(`
  query SearchUsersAndPostsInLib($query: String!) {
    search(input: { query: $query }) {
      ... on User {
        id
        name
        email
        role
      }
      ... on Post {
        id
        title
        status
        author {
          id
          name
        }
      }
    }
  }
`);

// Export for type checking
export { GetUsersWithFilters, SearchUsersAndPostsInLib };
