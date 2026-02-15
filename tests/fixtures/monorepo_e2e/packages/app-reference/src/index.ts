import { graphql } from './__generated__';

const ReferenceQuery = graphql(`
  query ReferenceQuery($first: Int = 10, $after: String) {
    me {
      id
      name
      email
      role
      isActive
      viewCount
      createdAt
      updatedAt
      preferences {
        theme
        notifications
        language
      }
      unmappedField
    }
    posts(first: $first, after: $after) {
      nodes {
        id
        title
        body
        author {
          id
          name
          email
          role
        }
        status
        viewCount
        tags
        createdAt
        updatedAt
      }
      pageInfo {
        hasNextPage
        endCursor
      }
      totalCount
    }
  }
`);

export { ReferenceQuery };
