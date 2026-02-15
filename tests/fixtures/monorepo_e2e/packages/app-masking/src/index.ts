import { graphql, getFragmentData } from './__generated__';

const UserCardFragment = graphql(`
  fragment UserCardFragment on User {
    id
    name
    email
    role
    createdAt
    ...UserPostsFragment
  }
`);

const UserPostsFragment = graphql(`
  fragment UserPostsFragment on User {
    posts(first: 3) {
      nodes {
        id
        title
        status
        viewCount
      }
    }
  }
`);

const GetUsersQuery = graphql(`
  query GetUsersWithMasking($first: Int = 10) {
    users(first: $first) {
      nodes {
        ...UserCardFragment
      }
    }
  }
`);

export function renderUser(user: any) {
  const userData = getFragmentData(UserCardFragment, user);
  const postsData = getFragmentData(UserPostsFragment, userData);
  
  return {
    name: userData.name,
    role: userData.role,
    postCount: postsData.posts?.nodes?.length || 0
  };
}
