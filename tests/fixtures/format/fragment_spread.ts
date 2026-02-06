const fragment = gql`fragment UserFields on User{id name email}`;
const query = gql`query{me{...UserFields}}`;
